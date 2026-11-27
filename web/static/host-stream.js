import { WebStreamServer } from "./pkg/sync2gether_web.js";
import { MseController } from "./mse.js";

const LABEL = "sync2gether-stream";
const ICE_SERVERS = [{ urls: ["stun:stun.cloudflare.com:3478", "stun:stun.l.google.com:19302"] }];
const GATHER_MS = 4000;
const HIGH_WATER = 1024 * 1024;
const LOW_WATER = 256 * 1024;
const THROUGHPUT_MS = 500;

function gathered(pc) {
  if (pc.iceGatheringState === "complete") return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, GATHER_MS);
    pc.addEventListener("icegatheringstatechange", () => {
      if (pc.iceGatheringState === "complete") {
        clearTimeout(timer);
        resolve();
      }
    });
  });
}

export class RemuxBridge {
  constructor() {
    this.worker = null;
    this.nextId = 1;
    this.pending = new Map();
    this.mse = null;
    this.skippedAudio = [];
    this.error = null;
  }

  ensureWorker() {
    if (this.worker) return this.worker;
    const worker = new Worker(new URL("./remux-worker.js", import.meta.url), { type: "module" });
    worker.onmessage = (event) => {
      const msg = event.data;
      const entry = this.pending.get(msg.id);
      if (!entry) return;
      this.pending.delete(msg.id);
      if (msg.ok) {
        entry.resolve(msg);
      } else {
        entry.reject(msg.error);
      }
    };
    worker.onerror = (err) => {
      const message = String(err && err.message || "Falha no worker de remux.");
      for (const entry of this.pending.values()) entry.reject(message);
      this.pending.clear();
    };
    this.worker = worker;
    return worker;
  }

  request(payload) {
    const worker = this.ensureWorker();
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      worker.postMessage({ ...payload, id });
    });
  }

  async open(file) {
    this.mse = null;
    this.skippedAudio = [];
    this.error = null;
    try {
      const res = await this.request({ type: "open", file });
      this.mse = res.mse;
      this.skippedAudio = res.skippedAudio || [];
      return { ok: true, mse: this.mse, skippedAudio: this.skippedAudio };
    } catch (error) {
      this.error = error;
      return { ok: false, error };
    }
  }

  async index() {
    const res = await this.request({ type: "index" });
    return res.bytes;
  }

  async init(track) {
    const res = await this.request({ type: "init", track });
    return res.bytes;
  }

  async segment(track, segment) {
    const res = await this.request({ type: "segment", track, segment });
    return res.bytes;
  }

  close() {
    for (const entry of this.pending.values()) entry.reject("Remux encerrado.");
    this.pending.clear();
    if (this.worker) {
      try { this.worker.postMessage({ type: "close" }); } catch {}
      this.worker.terminate();
      this.worker = null;
    }
    this.mse = null;
    this.skippedAudio = [];
    this.error = null;
  }
}

export class HostLocalMse {
  constructor({ remux, media, fileName, video, player, generation, onTracksChange, onError, ask }) {
    this.remux = remux;
    this.server = new WebStreamServer(media.bytes, 256 * 1024, new Uint8Array(0), true);
    this.closed = false;
    this.busy = false;
    const answer = {
      id: 1,
      media,
      file_name: fileName,
      duration: remux.mse.duration,
      chunk_size: 256 * 1024,
      hash_table_digest: "",
      mse: remux.mse,
      subtitle_tracks: [],
    };
    this.mse = new MseController({
      answer,
      video,
      player,
      guest: { stream_started: () => generation },
      send: (bytes) => {
        if (this.closed) return false;
        try {
          this.server.on_request(bytes);
          this.pump();
          return true;
        } catch {
          return false;
        }
      },
      fail: (msg) => {
        if (!this.closed) onError?.(msg);
      },
      onTracksChange,
      ask,
    });
    this.mse.onOpen();
  }

  audioTracks() {
    return this.mse ? this.mse.tracks : [];
  }

  activeAudioTrack() {
    return this.mse ? this.mse.activeTrack : null;
  }

  selectAudioTrack(index) {
    return this.mse ? this.mse.selectAudioTrack(index) : false;
  }

  pump() {
    if (this.closed || !this.server) return;
    while (this.server.has_outgoing()) {
      const frag = this.server.take_fragment();
      if (!frag) break;
      this.mse.onMessage(frag.buffer.slice(frag.byteOffset, frag.byteOffset + frag.byteLength));
    }
    if (this.busy) return;
    const action = JSON.parse(this.server.next_action());
    if (!action) return;
    this.busy = true;
    this.runAction(action).finally(() => {
      this.busy = false;
      this.pump();
    });
  }

  async runAction(action) {
    if (this.closed || !this.server) return;
    try {
      if (action.kind === "remux_index") {
        const bytes = await this.remux.index();
        if (!this.closed) this.server.provide_mse_index(bytes);
      } else if (action.kind === "remux_init") {
        const bytes = await this.remux.init(action.track);
        if (!this.closed) this.server.provide_mse_init(action.track, bytes);
      } else if (action.kind === "remux_segment") {
        const bytes = await this.remux.segment(action.track, action.segment);
        if (!this.closed) this.server.provide_mse_segment(action.track, action.segment, bytes);
      }
    } catch {
      if (this.closed) return;
      if (action.kind === "remux_index") this.server.fail_mse_index();
      else if (action.kind === "remux_init") this.server.fail_mse_init(action.track);
      else if (action.kind === "remux_segment") this.server.fail_mse_segment(action.track, action.segment);
    }
  }

  close() {
    this.closed = true;
    this.mse?.close();
    this.mse = null;
    if (this.server) {
      try { this.server.free(); } catch {}
      this.server = null;
    }
  }
}

export class HostStreamController {
  constructor({ now, getDuration, onAnswer, onStop, onStateChange }) {
    this.now = now;
    this.getDuration = getDuration;
    this.onAnswer = onAnswer;
    this.onStop = onStop;
    this.onStateChange = onStateChange;

    this.file = null;
    this.media = null;
    this.chunkSize = 256 * 1024;
    this.digest = "";
    this.hashTable = null;
    this.remux = null;

    this.sessionId = 0;
    this.pc = null;
    this.dc = null;
    this.server = null;
    this.busy = false;
    this.phase = "idle";
    this.error = null;
    this.lastBpsAt = 0;
    this.lastSentBytes = 0;
    this.bps = 0;
    this.hiddenWhileStreaming = false;
  }

  setFile({ file, media, chunkSize, digest, hashTable, remux }) {
    this.stopSession(false);
    this.file = file || null;
    this.media = media || null;
    this.chunkSize = chunkSize || 256 * 1024;
    this.digest = digest || "";
    this.hashTable = hashTable || null;
    this.remux = remux || null;
    this.phase = "idle";
    this.error = null;
    this.hiddenWhileStreaming = false;
  }

  active() {
    return this.phase === "connecting" || this.phase === "streaming";
  }

  noteHidden() {
    if (this.active()) {
      this.hiddenWhileStreaming = true;
      this.onStateChange?.();
    }
  }

  clearHiddenNotice() {
    this.hiddenWhileStreaming = false;
  }

  async handleOffer(id, sdp) {
    this.stopSession(false);
    if (!this.file || !this.media || !this.hashTable) {
      this.onStop?.(id);
      return;
    }
    const sessionId = id;
    this.sessionId = sessionId;
    this.phase = "connecting";
    this.error = null;
    this.bps = 0;
    this.lastSentBytes = 0;
    this.lastBpsAt = this.now();
    this.hiddenWhileStreaming = false;
    this.onStateChange?.();

    const remuxEnabled = Boolean(this.remux && this.remux.mse);
    const server = new WebStreamServer(
      this.media.bytes,
      this.chunkSize,
      this.hashTable,
      remuxEnabled,
    );
    this.server = server;

    const pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });
    this.pc = pc;

    pc.ondatachannel = (event) => {
      if (this.sessionId !== sessionId || event.channel.label !== LABEL) return;
      const dc = event.channel;
      dc.binaryType = "arraybuffer";
      dc.bufferedAmountLowThreshold = LOW_WATER;
      this.dc = dc;
      dc.onopen = () => {
        if (this.sessionId !== sessionId) return;
        this.phase = "streaming";
        this.onStateChange?.();
        this.pump();
      };
      dc.onbufferedamountlow = () => {
        if (this.sessionId !== sessionId) return;
        this.pump();
      };
      dc.onmessage = (msg) => {
        if (this.sessionId !== sessionId || !(msg.data instanceof ArrayBuffer) || !this.server) return;
        try {
          this.server.on_request(new Uint8Array(msg.data));
        } catch {
          this.failSession("O convidado enviou um pedido de streaming inválido.");
          return;
        }
        this.pump();
      };
      dc.onclose = () => {
        if (this.sessionId !== sessionId) return;
        this.stopSession(false);
        this.onStateChange?.();
      };
    };

    pc.onconnectionstatechange = () => {
      if (this.sessionId !== sessionId) return;
      if (pc.connectionState === "failed") {
        this.failSession("Não foi possível conectar diretamente ao convidado.");
      } else if (pc.connectionState === "closed") {
        this.stopSession(false);
        this.onStateChange?.();
      }
    };

    try {
      await pc.setRemoteDescription({ type: "offer", sdp });
      await pc.setLocalDescription(await pc.createAnswer());
      await gathered(pc);
      if (this.sessionId !== sessionId || !pc.localDescription) return;
      const duration = this.remux?.mse?.duration || this.getDuration() || 0;
      const answer = {
        id: sessionId,
        sdp: pc.localDescription.sdp,
        media: this.media,
        file_name: this.file.name,
        duration: Number.isFinite(duration) && duration >= 0 ? duration : 0,
        chunk_size: this.chunkSize,
        hash_table_digest: this.digest,
        mse: this.remux?.mse || null,
        subtitle_tracks: [],
      };
      const sent = this.onAnswer?.(answer);
      if (!sent) {
        this.stopSession(false);
      }
      this.onStateChange?.();
    } catch {
      if (this.sessionId === sessionId) {
        this.failSession("Não foi possível preparar a conexão de streaming.");
      }
    }
  }

  handleRemoteStop(id) {
    if (this.sessionId === id) {
      this.stopSession(false);
      this.onStateChange?.();
    }
  }

  pump() {
    if (!this.server || !this.dc || this.dc.readyState !== "open") return;
    while (this.dc.bufferedAmount < HIGH_WATER && this.server.has_outgoing()) {
      const frag = this.server.take_fragment();
      if (!frag) break;
      try {
        this.dc.send(frag);
      } catch {
        this.failSession("A conexão direta de streaming caiu.");
        return;
      }
    }
    const time = this.now();
    if (time - this.lastBpsAt >= THROUGHPUT_MS) {
      const sent = this.server.sent_bytes();
      const elapsed = Math.max(1, time - this.lastBpsAt);
      this.bps = Math.round((sent - this.lastSentBytes) * 8000 / elapsed);
      this.lastSentBytes = sent;
      this.lastBpsAt = time;
    }
    if (this.busy || this.server.has_outgoing()) return;
    const action = JSON.parse(this.server.next_action());
    if (!action) {
      if (this.server.has_outgoing()) this.pump();
      return;
    }
    const sessionId = this.sessionId;
    this.busy = true;
    this.runAction(sessionId, action).finally(() => {
      if (this.sessionId === sessionId) {
        this.busy = false;
        this.pump();
      }
    });
  }

  async runAction(sessionId, action) {
    if (this.sessionId !== sessionId || !this.server) return;
    try {
      if (action.kind === "read_chunk") {
        const end = Math.min(this.file.size, action.offset + action.length);
        const bytes = new Uint8Array(await this.file.slice(action.offset, end).arrayBuffer());
        if (this.sessionId === sessionId && this.server) {
          this.server.provide_chunk(action.index, bytes);
        }
      } else if (action.kind === "remux_index") {
        const bytes = await this.remux.index();
        if (this.sessionId === sessionId && this.server) {
          this.server.provide_mse_index(bytes);
        }
      } else if (action.kind === "remux_init") {
        const bytes = await this.remux.init(action.track);
        if (this.sessionId === sessionId && this.server) {
          this.server.provide_mse_init(action.track, bytes);
        }
      } else if (action.kind === "remux_segment") {
        const bytes = await this.remux.segment(action.track, action.segment);
        if (this.sessionId === sessionId && this.server) {
          this.server.provide_mse_segment(action.track, action.segment, bytes);
        }
      }
    } catch {
      if (this.sessionId !== sessionId || !this.server) return;
      if (action.kind === "read_chunk") this.server.fail_chunk(action.index);
      else if (action.kind === "remux_index") this.server.fail_mse_index();
      else if (action.kind === "remux_init") this.server.fail_mse_init(action.track);
      else if (action.kind === "remux_segment") {
        this.server.fail_mse_segment(action.track, action.segment);
      }
    }
  }

  metrics() {
    const total = this.server ? this.server.total_chunks() : 0;
    const unique = this.server ? this.server.unique_chunks() : 0;
    return {
      phase: this.phase,
      bps: this.bps,
      sentBytes: this.server ? this.server.sent_bytes() : 0,
      uniqueChunks: unique,
      totalChunks: total,
      remux: Boolean(this.remux && this.remux.mse),
      hiddenWarning: this.hiddenWhileStreaming,
      error: this.error,
    };
  }

  describe(guestProgress) {
    if (this.phase === "connecting") {
      return "Conectando streaming P2P…";
    }
    if (this.phase === "failed") {
      return this.error || "Streaming interrompido.";
    }
    const bps = guestProgress?.transfer_bps || this.bps || 0;
    const mbps = (bps / 1e6).toFixed(1);
    const label = this.remux?.mse ? "Streaming adaptado" : "Enviando streaming P2P";
    if (guestProgress && guestProgress.total_bytes > 0) {
      const pct = Math.min(100, Math.round((guestProgress.cached_bytes / guestProgress.total_bytes) * 100));
      return `${label} (${pct}% · ${mbps} Mbit/s)`;
    }
    return `${label} (${mbps} Mbit/s)`;
  }

  failSession(message) {
    const id = this.sessionId;
    this.stopSession(false);
    this.phase = "failed";
    this.error = message;
    if (id) this.onStop?.(id);
    this.onStateChange?.();
  }

  stopSession(notify) {
    const id = this.sessionId;
    this.sessionId = 0;
    this.busy = false;
    if (this.dc) {
      this.dc.onopen = null;
      this.dc.onmessage = null;
      this.dc.onbufferedamountlow = null;
      this.dc.onclose = null;
      try { this.dc.close(); } catch {}
      this.dc = null;
    }
    if (this.pc) {
      this.pc.ondatachannel = null;
      this.pc.onconnectionstatechange = null;
      try { this.pc.close(); } catch {}
      this.pc = null;
    }
    if (this.server) {
      try { this.server.free(); } catch {}
      this.server = null;
    }
    if (this.phase !== "failed") {
      this.phase = "idle";
    }
    if (notify && id) {
      this.onStop?.(id);
    }
  }
}
