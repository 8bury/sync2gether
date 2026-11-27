const HEADER = 14;
const STATUS_OK = 0;
const OP_CANCEL = 3;
const OP_MSE_INDEX = 4;
const OP_MSE_INIT = 5;
const OP_MSE_SEGMENT = 6;
const NO_AUDIO_TRACK = 255;
const AHEAD_SEGMENTS = 3;
const MAX_MSE_SEGMENT_BYTES = 32 * 1024 * 1024;
const EVICT_BEHIND_SECS = 20;

function encodeCancel() {
  return new Uint8Array([OP_CANCEL]);
}

function encodeMseIndex() {
  return new Uint8Array([OP_MSE_INDEX]);
}

function encodeMseInit(track) {
  return new Uint8Array([OP_MSE_INIT, track & 0xff]);
}

function encodeMseSegment(track, segment) {
  const out = new Uint8Array(6);
  const view = new DataView(out.buffer);
  out[0] = OP_MSE_SEGMENT;
  out[1] = track & 0xff;
  view.setUint32(2, segment >>> 0, false);
  return out;
}

function decodeFragment(buffer) {
  if (buffer.byteLength < HEADER) return null;
  const view = new DataView(buffer);
  const ok = view.getUint8(0) === STATUS_OK;
  const kind = view.getUint8(1);
  const index = view.getUint32(2, false);
  const offset = view.getUint32(6, false);
  const total = view.getUint32(10, false);
  const data = new Uint8Array(buffer, HEADER);
  return { ok, kind, index, offset, total, data };
}

function decodeIndex(bytes) {
  if (bytes.byteLength < 4) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const count = view.getUint32(0, false);
  if (bytes.byteLength !== 4 + count * 12) return null;
  const out = [];
  for (let i = 0; i < count; i++) {
    const base = 4 + i * 12;
    const hi = view.getUint32(base, false);
    const lo = view.getUint32(base + 4, false);
    const startMs = hi * 4294967296 + lo;
    const durationMs = view.getUint32(base + 8, false);
    out.push({ startMs, durationMs });
  }
  return out;
}

export class MseController {
  constructor({ answer, video, player, guest, send, fail, onTracksChange, ask }) {
    this.answer = answer;
    this.mse = answer.mse;
    this.video = video;
    this.player = player;
    this.guest = guest;
    this.send = send;
    this.fail = fail;
    this.onTracksChange = onTracksChange;
    this.ask = ask;

    this.tracks = Array.isArray(this.mse.audio_tracks) ? this.mse.audio_tracks : [];
    this.activeTrack = this.mse.default_audio !== null && this.mse.default_audio !== undefined
      ? this.mse.default_audio
      : NO_AUDIO_TRACK;

    this.segments = null;
    this.initCache = new Map();
    this.initialSegment = null;
    this.assembly = null;
    this.inFlight = null;
    this.pendingQueue = [];
    this.appended = new Set();
    this.queuedAppend = new Set();
    this.receivedBytes = 0;
    this.started = false;
    this.closed = false;
    this.needInitAppend = true;
    this.clearAfterSwitch = false;
    this.endedSignaled = false;

    this.mediaSource = null;
    this.objectUrl = null;
    this.sourceBuffer = null;
    this.appendQueue = [];

    this._onSeeking = () => this.onSeek();
    this._onTimeUpdate = () => this.pump();
    this.video.addEventListener("seeking", this._onSeeking);
    this.video.addEventListener("timeupdate", this._onTimeUpdate);
  }

  onOpen() {
    if (this.closed) return;
    if (!this.segments) {
      this.enqueueRequest({ kind: OP_MSE_INDEX, index: 0, bytes: encodeMseIndex() });
    }
    if (!this.initCache.has(this.activeTrack)) {
      this.enqueueRequest({
        kind: OP_MSE_INIT,
        index: this.activeTrack,
        bytes: encodeMseInit(this.activeTrack),
      });
    }
    this.flushRequests();
  }

  enqueueRequest(req) {
    this.pendingQueue.push(req);
  }

  flushRequests() {
    if (this.closed || this.inFlight || this.pendingQueue.length === 0) return;
    const next = this.pendingQueue.shift();
    if (!this.send(next.bytes)) {
      this.pendingQueue.unshift(next);
      return;
    }
    this.inFlight = next;
  }

  onMessage(buffer) {
    if (this.closed) return;
    const frag = decodeFragment(buffer);
    if (!frag) {
      this.fail("O anfitrião enviou dados de streaming inválidos.", true);
      return;
    }
    if (!this.inFlight || frag.kind !== this.inFlight.kind || frag.index !== this.inFlight.index) {
      return;
    }
    if (!frag.ok) {
      this.inFlight = null;
      this.fail("O anfitrião não conseguiu preparar este trecho do filme.", true);
      return;
    }
    if (frag.total > MAX_MSE_SEGMENT_BYTES) {
      this.inFlight = null;
      this.fail("Segmento de streaming maior que o limite permitido.", true);
      return;
    }
    if (frag.offset === 0 || !this.assembly || this.assembly.kind !== frag.kind || this.assembly.index !== frag.index) {
      if (frag.offset !== 0) return;
      this.assembly = {
        kind: frag.kind,
        index: frag.index,
        buf: new Uint8Array(frag.total),
        received: 0,
      };
    }
    if (frag.offset !== this.assembly.received || frag.offset + frag.data.byteLength > this.assembly.buf.byteLength) {
      this.assembly = null;
      return;
    }
    this.assembly.buf.set(frag.data, frag.offset);
    this.assembly.received += frag.data.byteLength;
    if (this.assembly.received < this.assembly.buf.byteLength) return;

    const payload = this.assembly.buf;
    const done = this.inFlight;
    this.assembly = null;
    this.inFlight = null;
    this.receivedBytes += payload.byteLength;

    if (done.kind === OP_MSE_INDEX) {
      const parsed = decodeIndex(payload);
      if (!parsed || parsed.length === 0) {
        this.fail("O índice de busca do filme é inválido.", true);
        return;
      }
      this.segments = parsed;
      this.maybeStart();
    } else if (done.kind === OP_MSE_INIT) {
      this.initCache.set(done.index, payload);
      if (done.index === this.activeTrack) {
        if (!this.started) {
          this.maybeStart();
        } else if (this.needInitAppend) {
          this.needInitAppend = false;
          this.queueAppend({ type: "init", data: payload, clearAhead: this.clearAfterSwitch });
          this.clearAfterSwitch = false;
        }
      }
    } else if (done.kind === OP_MSE_SEGMENT) {
      const track = (done.index >>> 24) & 0xff;
      const segIdx = done.index & 0x00ffffff;
      if (track === this.activeTrack) {
        if (!this.started) {
          this.initialSegment = { segIdx, data: payload };
          this.maybeStart();
        } else {
          if (this.needInitAppend && this.initCache.has(track)) {
            this.needInitAppend = false;
            this.queueAppend({
              type: "init",
              data: this.initCache.get(track),
              clearAhead: this.clearAfterSwitch,
            });
            this.clearAfterSwitch = false;
          }
          this.appended.add(segIdx);
          this.queuedAppend.add(segIdx);
          this.queueAppend({ type: "segment", segIdx, data: payload });
        }
      }
    }
    this.pump();
    this.flushRequests();
  }

  maybeStart() {
    if (this.started || !this.segments || !this.initCache.has(this.activeTrack)) return;
    if (!this.initialSegment) {
      if (!this.isQueuedOrInFlight(0)) {
        this.requestSegment(0);
      }
      return;
    }
    this.started = true;
    const ms = new MediaSource();
    this.mediaSource = ms;
    this.objectUrl = URL.createObjectURL(ms);
    ms.addEventListener("sourceopen", () => {
      if (this.closed || this.mediaSource !== ms) return;
      try {
        if (Number.isFinite(this.mse.duration) && this.mse.duration > 0) {
          ms.duration = this.mse.duration;
        }
        const sb = ms.addSourceBuffer(this.mse.mime);
        sb.mode = "segments";
        this.sourceBuffer = sb;
        sb.addEventListener("updateend", () => {
          if (this.appendingSeg !== null && this.appendingSeg !== undefined) {
            this.queuedAppend.delete(this.appendingSeg);
            this.appendingSeg = null;
          }
          this.drainAppend();
          this.pump();
        });
        sb.addEventListener("error", () => {
          if (!this.closed) {
            this.fail("O navegador não conseguiu decodificar o fluxo deste arquivo.", true);
          }
        });
        this.needInitAppend = false;
        this.queueAppend({ type: "init", data: this.initCache.get(this.activeTrack), clearAhead: false });
        if (this.initialSegment) {
          const { segIdx, data } = this.initialSegment;
          this.initialSegment = null;
          this.appended.add(segIdx);
          this.queuedAppend.add(segIdx);
          this.queueAppend({ type: "segment", segIdx, data });
        }
        this.pump();
        this.flushRequests();
      } catch {
        this.fail("Este navegador não suporta o formato de vídeo deste arquivo. Use o app.", true);
      }
    }, { once: true });

    const generation = this.guest.stream_started(
      this.answer.media.hash,
      this.answer.media.bytes,
      this.answer.file_name,
    );
    this.player.load(this.objectUrl, generation);
    this.onTracksChange?.(this.tracks, this.activeTrack);
    this.ask();
  }

  queueAppend(op) {
    this.appendQueue.push(op);
    this.drainAppend();
  }

  drainAppend() {
    if (this.closed || !this.sourceBuffer || this.sourceBuffer.updating) return;
    if (this.mediaSource?.readyState !== "open") return;

    const cur = this.video.currentTime || 0;
    const buffered = this.sourceBuffer.buffered;
    if (cur > EVICT_BEHIND_SECS + 10 && buffered.length > 0 && buffered.start(0) < cur - EVICT_BEHIND_SECS) {
      try {
        this.sourceBuffer.remove(0, cur - EVICT_BEHIND_SECS + 5);
        return;
      } catch {}
    }

    const op = this.appendQueue.shift();
    if (!op) {
      if (
        this.segments
        && this.appended.has(this.segments.length - 1)
        && !this.inFlight
        && this.pendingQueue.length === 0
        && !this.endedSignaled
        && !this.video.seeking
      ) {
        try {
          if (this.mediaSource.readyState === "open") {
            this.mediaSource.endOfStream();
            this.endedSignaled = true;
          }
        } catch {}
      }
      return;
    }

    if (op.type === "clear") {
      try {
        if (op.end > op.start) {
          this.sourceBuffer.remove(op.start, op.end);
          return;
        }
      } catch {}
      this.drainAppend();
      return;
    }

    if (op.clearAhead && buffered.length > 0) {
      const end = buffered.end(buffered.length - 1);
      if (end > cur + 0.5) {
        this.appendQueue.unshift({ ...op, clearAhead: false });
        try {
          this.sourceBuffer.remove(cur + 0.5, end + 1);
          return;
        } catch {}
      }
    }

    try {
      this.endedSignaled = false;
      this.appendingSeg = op.type === "segment" ? op.segIdx : null;
      this.sourceBuffer.appendBuffer(op.data);
    } catch (err) {
      if (err && err.name === "QuotaExceededError" && buffered.length > 0) {
        this.appendQueue.unshift(op);
        const start = buffered.start(0);
        const trimTo = Math.min(cur - 2, start + 30);
        if (trimTo > start) {
          try {
            this.sourceBuffer.remove(start, trimTo);
            return;
          } catch {}
        }
      }
      this.fail("Não foi possível carregar o próximo trecho do vídeo no navegador.", true);
    }
  }

  segmentForTime(seconds) {
    if (!this.segments || this.segments.length === 0) return 0;
    const ms = Math.max(0, seconds * 1000);
    let lo = 0;
    let hi = this.segments.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >>> 1;
      if (this.segments[mid].startMs <= ms) {
        lo = mid;
      } else {
        hi = mid - 1;
      }
    }
    return lo;
  }

  isSegmentBuffered(segIdx) {
    if (!this.sourceBuffer || !this.segments || segIdx >= this.segments.length) return false;
    const seg = this.segments[segIdx];
    const startSec = seg.startMs / 1000;
    const probe = startSec + Math.min(0.25, (seg.durationMs / 1000) * 0.5);
    const buffered = this.sourceBuffer.buffered;
    for (let i = 0; i < buffered.length; i++) {
      if (buffered.start(i) <= probe + 0.05 && buffered.end(i) >= probe) {
        return true;
      }
    }
    return false;
  }

  isQueuedOrInFlight(segIdx) {
    const key = ((this.activeTrack & 0xff) << 24) | (segIdx & 0x00ffffff);
    if (this.inFlight && this.inFlight.kind === OP_MSE_SEGMENT && this.inFlight.index === key) {
      return true;
    }
    return this.pendingQueue.some((req) => req.kind === OP_MSE_SEGMENT && req.index === key);
  }

  onSeek() {
    if (this.closed || !this.segments || !this.sourceBuffer) return;
    const targetIdx = this.segmentForTime(this.video.currentTime || 0);
    if (this.isSegmentBuffered(targetIdx)) {
      this.pump();
      return;
    }
    this.cancelNetwork();
    this.requestSegment(targetIdx);
    this.pump();
    this.flushRequests();
  }

  cancelNetwork() {
    this.send(encodeCancel());
    this.inFlight = null;
    this.assembly = null;
    this.pendingQueue = [];
  }

  requestSegment(segIdx) {
    const key = ((this.activeTrack & 0xff) << 24) | (segIdx & 0x00ffffff);
    this.enqueueRequest({
      kind: OP_MSE_SEGMENT,
      index: key,
      bytes: encodeMseSegment(this.activeTrack, segIdx),
    });
  }

  selectAudioTrack(trackIndex) {
    if (this.closed || !this.segments) return false;
    if (!this.tracks.some((t) => t.index === trackIndex)) return false;
    if (trackIndex === this.activeTrack) return true;
    this.activeTrack = trackIndex;
    this.cancelNetwork();
    this.appended.clear();
    this.queuedAppend.clear();
    this.appendQueue = [];
    this.needInitAppend = true;
    this.clearAfterSwitch = true;
    this.endedSignaled = false;
    if (this.sourceBuffer && !this.sourceBuffer.updating) {
      try { this.sourceBuffer.abort(); } catch {}
    }
    if (!this.initCache.has(trackIndex)) {
      this.enqueueRequest({
        kind: OP_MSE_INIT,
        index: trackIndex,
        bytes: encodeMseInit(trackIndex),
      });
    }
    const targetIdx = this.segmentForTime(this.video.currentTime || 0);
    this.requestSegment(targetIdx);
    for (let i = 1; i <= 2; i++) {
      if (targetIdx + i < this.segments.length) {
        this.requestSegment(targetIdx + i);
      }
    }
    this.flushRequests();
    this.onTracksChange?.(this.tracks, this.activeTrack);
    return true;
  }

  pump() {
    if (this.closed || !this.segments || !this.sourceBuffer) return;
    const cur = this.video.currentTime || 0;
    const baseIdx = this.segmentForTime(cur);
    const limit = Math.min(this.segments.length, baseIdx + AHEAD_SEGMENTS + 1);
    for (let idx = baseIdx; idx < limit; idx++) {
      if (!this.isSegmentBuffered(idx) && !this.isQueuedOrInFlight(idx) && !this.queuedAppend.has(idx)) {
        this.requestSegment(idx);
      }
    }
    this.flushRequests();
  }

  progress(transferBps, reconnecting) {
    const total = this.answer.media.bytes || 0;
    const duration = this.mse.duration || this.video.duration || 0;
    const ranges = [];
    let bufferedSecs = 0;
    let aheadSecs = 0;
    const cur = this.video.currentTime || 0;
    if (this.sourceBuffer && duration > 0) {
      const b = this.sourceBuffer.buffered;
      for (let i = 0; i < b.length; i++) {
        const s = b.start(i);
        const e = b.end(i);
        bufferedSecs += Math.max(0, e - s);
        if (s <= cur + 0.1 && e >= cur) {
          aheadSecs = Math.max(0, e - cur);
        }
        const ps = Math.max(0, Math.min(999, Math.floor((s / duration) * 1000)));
        const pe = Math.max(ps + 1, Math.min(1000, Math.ceil((e / duration) * 1000)));
        if (ranges.length < 64) ranges.push([ps, pe]);
      }
    }
    const ratio = duration > 0 ? Math.min(1, bufferedSecs / duration) : 0;
    const aheadRatio = duration > 0 ? Math.min(1, aheadSecs / duration) : 0;
    return JSON.stringify({
      cached_bytes: Math.min(total, Math.round(total * ratio)),
      total_bytes: total,
      transfer_bps: transferBps,
      link_bps: null,
      buffered_ahead_bytes: Math.min(total, Math.round(total * aheadRatio)),
      reconnecting,
      complete: false,
      cached_ranges: ranges,
    });
  }

  close() {
    this.closed = true;
    this.video.removeEventListener("seeking", this._onSeeking);
    this.video.removeEventListener("timeupdate", this._onTimeUpdate);
    this.pendingQueue = [];
    this.appendQueue = [];
    this.inFlight = null;
    this.initialSegment = null;
    if (this.objectUrl) {
      URL.revokeObjectURL(this.objectUrl);
      this.objectUrl = null;
    }
    this.mediaSource = null;
    this.sourceBuffer = null;
    this.onTracksChange?.([], null);
  }
}
