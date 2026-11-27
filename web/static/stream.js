import { WebStreamClient } from "./pkg/sync2gether_web.js";
import { MseController } from "./mse.js";

const LABEL = "sync2gether-stream";
const ICE_SERVERS = [{ urls: ["stun:stun.cloudflare.com:3478", "stun:stun.l.google.com:19302"] }];
const GATHER_MS = 4000;
const ANSWER_MS = 20000;
const PROGRESS_MS = 500;
const MIME = {
  mp4: "video/mp4",
  m4v: "video/mp4",
  mov: "video/mp4",
  webm: "video/webm",
  mkv: "video/x-matroska",
  ogv: "video/ogg",
};

let current = null;
let worker = null;

function mimeFor(name) {
  const extension = (name.split(".").pop() || "").toLowerCase();
  return MIME[extension] || "video/mp4";
}

function budget() {
  const mobile = /Mobi|Android|iPhone|iPad/.test(navigator.userAgent);
  const small = typeof navigator.deviceMemory === "number" && navigator.deviceMemory <= 4;
  return (mobile || small ? 128 : 256) * 1024 * 1024;
}

function splitRequests(bytes) {
  const out = [];
  let i = 0;
  while (i < bytes.length) {
    const size = bytes[i] === 2 ? 5 : 1;
    out.push(bytes.slice(i, i + size));
    i += size;
  }
  return out;
}

async function serviceWorker() {
  if (!("serviceWorker" in navigator)) {
    throw new Error("Este navegador não permite streaming nesta página. Use sua cópia do filme ou o app.");
  }
  if (!worker) {
    navigator.serviceWorker.addEventListener("message", (event) => {
      const port = event.ports[0];
      if (!port || !event.data || event.data.type !== "range") return;
      if (current && current.id === event.data.id) {
        current.range(event.data, port);
      } else {
        port.postMessage({ missing: true });
      }
    });
    worker = navigator.serviceWorker.register("sw.js", { scope: "./" });
  }
  await worker;
  await navigator.serviceWorker.ready;
  if (navigator.serviceWorker.controller) return;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("Não foi possível preparar o streaming nesta aba. Recarregue a página e tente de novo.")), 5000);
    navigator.serviceWorker.addEventListener("controllerchange", () => {
      clearTimeout(timer);
      resolve();
    }, { once: true });
  });
}

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

class Stream {
  constructor({ guest, player, pump, now, onTracksChange }) {
    this.guest = guest;
    this.player = player;
    this.ask = pump;
    this.now = now;
    this.onTracksChange = onTracksChange;
    this.id = (crypto.getRandomValues(new Uint32Array(1))[0] >>> 1) + 1;
    this.phase = "preparing";
    this.error = null;
    this.client = null;
    this.mse = null;
    this.answer = null;
    this.started = false;
    this.pending = [];
    this.pc = null;
    this.dc = null;
    this.timer = null;
    this.lastProgress = 0;
    this.lastBytes = 0;
    this.bps = 0;
  }

  async start() {
    current = this;
    try {
      const pc = new RTCPeerConnection({ iceServers: ICE_SERVERS });
      this.pc = pc;
      const dc = pc.createDataChannel(LABEL, { ordered: true });
      dc.binaryType = "arraybuffer";
      this.dc = dc;
      dc.onopen = () => {
        this.phase = "streaming";
        if (this.mse) {
          this.mse.onOpen();
        } else {
          this.flush();
        }
        this.ask();
      };
      dc.onmessage = (event) => {
        if (!(event.data instanceof ArrayBuffer)) return;
        if (this.mse) {
          this.mse.onMessage(event.data);
          this.started = this.mse.started;
          return;
        }
        if (!this.client) return;
        this.client.on_message(new Uint8Array(event.data));
        const error = this.client.error();
        if (error) {
          this.fail(error, true);
          return;
        }
        this.begin();
        this.serve();
        this.flush();
      };
      dc.onclose = () => this.fail("A conexão direta com o anfitrião caiu. Escolha sua cópia ou tente o streaming de novo.", true);
      pc.onconnectionstatechange = () => {
        if (pc.connectionState === "failed") {
          this.fail("Não foi possível conectar diretamente ao anfitrião. A rede de um dos dois bloqueia conexões diretas; use sua cópia do filme ou o app.", true);
        }
      };
      await pc.setLocalDescription(await pc.createOffer());
      await gathered(pc);
      if (this.phase === "failed") return;
      if (!this.guest.stream_offer(this.id, pc.localDescription.sdp, this.now())) {
        throw new Error("O anfitrião não está oferecendo streaming agora.");
      }
      this.phase = "negotiating";
      this.timer = setTimeout(() => {
        if (this.phase === "negotiating") this.fail("O anfitrião não respondeu ao pedido de streaming.", true);
      }, ANSWER_MS);
      this.ask();
    } catch (error) {
      this.fail(String(error && error.message || error), true);
    }
  }

  command(command) {
    if (command.kind === "stream_answer" && command.answer.id === this.id) {
      this.accept(command.answer);
    } else if (command.kind === "stream_stopped" && command.id === this.id) {
      this.fail("O anfitrião encerrou o streaming.", false);
    }
  }

  async accept(answer) {
    if (this.phase !== "negotiating") return;
    clearTimeout(this.timer);
    this.answer = answer;
    if (answer.mse) {
      if (typeof MediaSource === "undefined" || !MediaSource.isTypeSupported(answer.mse.mime)) {
        this.fail("Este navegador não suporta o codec de vídeo deste arquivo. Use o app de desktop.", true);
        return;
      }
      this.mse = new MseController({
        answer,
        video: this.player.video,
        player: this.player,
        guest: this.guest,
        send: (bytes) => {
          if (!this.dc || this.dc.readyState !== "open") return false;
          this.dc.send(bytes);
          return true;
        },
        fail: (msg, notify) => this.fail(msg, notify),
        onTracksChange: (tracks, active) => this.onTracksChange?.(tracks, active),
        ask: () => {
          this.started = Boolean(this.mse?.started);
          this.ask();
        },
      });
    } else {
      try {
        await serviceWorker();
      } catch (error) {
        this.fail(String(error && error.message || error), true);
        return;
      }
      if (this.phase === "failed") return;
      this.client = new WebStreamClient(answer.media.bytes, answer.chunk_size, answer.hash_table_digest, budget());
    }
    this.phase = "connecting";
    try {
      await this.pc.setRemoteDescription({ type: "answer", sdp: answer.sdp });
    } catch {
      this.fail("A resposta de streaming do anfitrião não pôde ser usada.", true);
    }
    this.ask();
  }

  audioTracks() {
    return this.mse ? this.mse.tracks : [];
  }

  activeAudioTrack() {
    return this.mse ? this.mse.activeTrack : null;
  }

  selectAudioTrack(trackIndex) {
    return this.mse ? this.mse.selectAudioTrack(trackIndex) : false;
  }

  flush() {
    if (!this.client || !this.dc || this.dc.readyState !== "open") return;
    for (const request of splitRequests(this.client.take_requests())) {
      this.dc.send(request);
    }
  }

  begin() {
    if (this.started || !this.client || !this.client.ready()) return;
    this.started = true;
    const answer = this.answer;
    const generation = this.guest.stream_started(answer.media.hash, answer.media.bytes, answer.file_name);
    this.player.load(`stream/${this.id}/${encodeURIComponent(answer.file_name)}`, generation);
    this.ask();
  }

  range(request, port) {
    this.pending.push({ start: request.start, end: request.end, port });
    this.serve();
    this.flush();
  }

  serve() {
    if (!this.client) return;
    const total = this.client.file_size();
    const mime = mimeFor(this.answer.file_name);
    this.pending = this.pending.filter(({ start, end, port }) => {
      const bytes = this.client.read(start, end);
      if (!bytes) return true;
      port.postMessage({ bytes, total, mime }, [bytes.buffer]);
      return false;
    });
  }

  pump() {
    if ((!this.client && !this.mse) || this.phase === "failed") return;
    this.mse?.pump();
    const time = this.now();
    if (time - this.lastProgress < PROGRESS_MS) return;
    const elapsed = Math.max(1, time - this.lastProgress);
    const received = this.mse ? this.mse.receivedBytes : this.client.received_bytes();
    this.bps = this.lastProgress ? Math.round((received - this.lastBytes) * 8000 / elapsed) : 0;
    this.lastProgress = time;
    this.lastBytes = received;
    const progress = this.mse
      ? this.mse.progress(this.bps, this.phase !== "streaming")
      : this.client.progress(this.bps, this.phase !== "streaming");
    this.guest.stream_progress(progress);
  }

  describe() {
    switch (this.phase) {
      case "preparing":
      case "negotiating":
        return "Pedindo o streaming ao anfitrião…";
      case "connecting":
        return "Conectando direto ao computador do anfitrião…";
      case "streaming": {
        const mbps = (this.bps / 1e6).toFixed(1);
        if (this.mse) {
          return this.started
            ? `Streaming adaptado (${mbps} Mbit/s)`
            : "Preparando o fluxo de vídeo e áudio…";
        }
        return this.started ? `Streaming direto do anfitrião (${mbps} Mbit/s).` : "Recebendo a lista de blocos…";
      }
      default:
        return this.error || "Streaming encerrado.";
    }
  }

  close() {
    clearTimeout(this.timer);
    this.mse?.close();
    this.mse = null;
    for (const { port } of this.pending) port.postMessage({ missing: true });
    this.pending = [];
    if (this.dc) {
      this.dc.onclose = null;
      this.dc.onmessage = null;
    }
    if (this.pc) {
      this.pc.onconnectionstatechange = null;
      try { this.pc.close(); } catch {}
    }
    if (current === this) current = null;
  }

  fail(message, notify) {
    if (this.phase === "failed") return;
    this.phase = "failed";
    this.error = message;
    if (notify) this.guest.stop_stream(this.now());
    if (this.started) this.player.unload(this.guest.generation());
    this.close();
    this.ask();
  }

  stop() {
    if (this.phase !== "failed") {
      this.phase = "failed";
      this.error = "Streaming encerrado.";
      this.guest.stop_stream(this.now());
      if (this.started) this.player.unload(this.guest.generation());
    }
    this.close();
  }
}

export function createStream(context) {
  const stream = new Stream(context);
  stream.start();
  return stream;
}
