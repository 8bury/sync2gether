import { WebHost } from "./pkg/sync2gether_web.js";
import { now } from "./player.js";
import { setEmblem } from "./ui.js";

const $ = (id) => document.getElementById(id);
const HOST_STORE = "sync2gether.host";
const RISKY_FORMATS = /\.(mkv|avi|wmv|flv|ts|m2ts|vob|rmvb)$/i;

function hex(bytes) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function unhex(text) {
  const pairs = text.match(/[0-9a-f]{2}/gi) || [];
  return new Uint8Array(pairs.map((p) => parseInt(p, 16)));
}

export function loadSavedHost() {
  try {
    return JSON.parse(sessionStorage.getItem(HOST_STORE) || "null");
  } catch {
    return null;
  }
}

export function clearSavedHost() {
  try {
    sessionStorage.removeItem(HOST_STORE);
  } catch {}
}

function saveHost(host) {
  try {
    const key = host.resume_key();
    sessionStorage.setItem(
      HOST_STORE,
      JSON.stringify({
        room: host.room() || null,
        secret: host.secret(),
        words: Array.from(host.words()),
        key: key ? hex(key) : null,
      }),
    );
  } catch {}
}

export function roomLink(code) {
  if (!code) return "";
  return `${location.origin}${location.pathname}#${code}`;
}

export function copyWithFeedback(value, button) {
  if (!value || !button) return;
  const fallback = () => {
    const area = document.createElement("textarea");
    area.value = value;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    try { document.execCommand("copy"); } catch {}
    area.remove();
  };
  const done = () => {
    if (!button.dataset.label) {
      button.dataset.label = button.textContent;
    }
    button.textContent = "Copiado!";
    clearTimeout(Number(button.dataset.timer || 0));
    button.dataset.timer = String(
      setTimeout(() => {
        button.textContent = button.dataset.label || button.textContent;
      }, 2000),
    );
  };
  if (navigator.clipboard && navigator.clipboard.writeText) {
    navigator.clipboard.writeText(value).then(done, () => {
      fallback();
      done();
    });
  } else {
    fallback();
    done();
  }
}

export class HostController {
  constructor({ player, relayUrl, onView, onStreamOffer, onStreamStop, onTick }) {
    this.player = player;
    this.relayUrl = relayUrl;
    this.onView = onView;
    this.onStreamOffer = onStreamOffer;
    this.onStreamStop = onStreamStop;
    this.onTick = onTick;
    this.host = null;
    this.controlSocket = null;
    this.controlConnectAt = null;
    this.acceptSockets = new Map();
  }

  start(name, saved) {
    this.closeAll();
    if (saved && saved.secret && Array.isArray(saved.words) && saved.words.length === 2) {
      this.host = WebHost.restore(
        name,
        saved.secret,
        saved.words[0],
        saved.words[1],
        saved.room || undefined,
        saved.key ? unhex(saved.key) : undefined,
      );
    } else {
      this.host = new WebHost(name);
    }
    saveHost(this.host);
    this.host.start(now());
    this.pump();
  }

  active() {
    return this.host !== null;
  }

  call(action) {
    if (!this.host) return;
    action(this.host);
    this.pump();
  }

  tick() {
    if (!this.host) return;
    if (this.controlConnectAt !== null && !this.controlSocket && now() >= this.controlConnectAt) {
      this.controlConnectAt = null;
      this.connectControl();
    }
    this.host.player_status(JSON.stringify(this.player.status()), now());
    this.host.tick(now());
    this.onTick?.();
    this.pump();
  }

  pump() {
    const host = this.host;
    if (!host) return;
    const ctrlOut = host.take_control_output();
    if (ctrlOut.length && this.controlSocket && this.controlSocket.readyState === WebSocket.OPEN) {
      this.controlSocket.send(ctrlOut);
    }
    for (const [id, socket] of this.acceptSockets.entries()) {
      const out = host.take_accept_output(id);
      if (out.length && socket.readyState === WebSocket.OPEN) {
        socket.send(out);
      }
    }
    for (const command of JSON.parse(host.take_commands())) {
      if (command.kind === "connect_control") {
        this.controlConnectAt = command.at;
      } else if (command.kind === "close_control") {
        this.closeControl();
      } else if (command.kind === "open_accept") {
        this.openAccept(command.id);
      } else if (command.kind === "close_accept") {
        const socket = this.acceptSockets.get(command.id);
        if (socket && socket.readyState === WebSocket.OPEN) {
          const tail = host.take_accept_output(command.id);
          if (tail.length) socket.send(tail);
        }
        this.closeAccept(command.id);
      } else if (command.kind === "web_stream_offer") {
        this.onStreamOffer?.(command.id, command.sdp);
      } else if (command.kind === "stream_stop") {
        this.onStreamStop?.(command.id);
      } else {
        this.player.run(command);
      }
    }
    saveHost(host);
    this.onView(JSON.parse(host.view()));
  }

  connectControl() {
    const socket = new WebSocket(this.relayUrl());
    socket.binaryType = "arraybuffer";
    this.controlSocket = socket;
    socket.onopen = () => {
      if (this.controlSocket !== socket) return;
      this.call((h) => h.control_open(now()));
    };
    socket.onmessage = (event) => {
      if (this.controlSocket !== socket || !(event.data instanceof ArrayBuffer)) return;
      this.call((h) => h.control_data(new Uint8Array(event.data), now()));
    };
    const closed = () => {
      if (this.controlSocket !== socket) return;
      this.controlSocket = null;
      this.call((h) => h.control_closed(now()));
    };
    socket.onclose = closed;
    socket.onerror = closed;
  }

  closeControl() {
    const socket = this.controlSocket;
    this.controlSocket = null;
    if (socket) {
      socket.onclose = null;
      socket.onerror = null;
      socket.onmessage = null;
      try { socket.close(); } catch {}
    }
  }

  openAccept(id) {
    const socket = new WebSocket(this.relayUrl());
    socket.binaryType = "arraybuffer";
    this.acceptSockets.set(id, socket);
    socket.onopen = () => {
      if (this.acceptSockets.get(id) !== socket) return;
      this.call((h) => h.accept_open(id, now()));
    };
    socket.onmessage = (event) => {
      if (this.acceptSockets.get(id) !== socket || !(event.data instanceof ArrayBuffer)) return;
      this.call((h) => h.accept_data(id, new Uint8Array(event.data), now()));
    };
    const closed = () => {
      if (this.acceptSockets.get(id) !== socket) return;
      this.acceptSockets.delete(id);
      this.call((h) => h.accept_closed(id, now()));
    };
    socket.onclose = closed;
    socket.onerror = closed;
  }

  closeAccept(id) {
    const socket = this.acceptSockets.get(id);
    this.acceptSockets.delete(id);
    if (socket) {
      socket.onclose = null;
      socket.onerror = null;
      socket.onmessage = null;
      try { socket.close(); } catch {}
    }
  }

  closeAll() {
    this.controlConnectAt = null;
    this.closeControl();
    for (const id of Array.from(this.acceptSockets.keys())) {
      this.closeAccept(id);
    }
  }

  leave() {
    if (this.host) {
      this.host.leave(now());
      for (const [id, socket] of this.acceptSockets.entries()) {
        const out = this.host.take_accept_output(id);
        if (out.length && socket.readyState === WebSocket.OPEN) {
          socket.send(out);
        }
      }
      this.closeAll();
      this.host = null;
    }
    clearSavedHost();
  }

  pagehide() {
    if (!this.host) return;
    this.host.leave(now());
    for (const [id, socket] of this.acceptSockets.entries()) {
      const out = this.host.take_accept_output(id);
      if (out.length && socket.readyState === WebSocket.OPEN) {
        socket.send(out);
      }
    }
  }
}

export function formatBps(bps) {
  if (!Number.isFinite(bps) || bps <= 0) return "0.0 Mbit/s";
  if (bps >= 1e6) return `${(bps / 1e6).toFixed(1)} Mbit/s`;
  return `${Math.round(bps / 1e3)} kbit/s`;
}

export function buildHostStreamInfo(view, hostStream) {
  const m = hostStream ? hostStream.metrics() : null;
  const gp = view.guest_stream_progress || null;
  const active = Boolean(m?.phase === "connecting" || m?.phase === "streaming" || view.streaming || view.guest_streaming);
  const failed = Boolean(m?.phase === "failed" && m?.error);
  if (!active && !failed) return null;

  const phase = failed
    ? "failed"
    : m?.phase === "connecting" && !view.guest_streaming
    ? "connecting"
    : "streaming";
  const bps = gp?.transfer_bps || m?.bps || 0;
  let percent = 0;
  if (gp && gp.total_bytes > 0) {
    percent = Math.min(100, Math.round((gp.cached_bytes / gp.total_bytes) * 100));
  } else if (m && m.totalChunks > 0) {
    percent = Math.min(100, Math.round((m.uniqueChunks / m.totalChunks) * 100));
  }
  const remux = Boolean(m?.remux || view.host_web_remux);
  const title = phase === "failed"
    ? "Streaming P2P interrompido"
    : phase === "connecting"
    ? "Conectando streaming P2P…"
    : remux
    ? "Enviando streaming adaptado (MKV)"
    : "Enviando por streaming P2P";
  const metricsParts = [];
  if (phase === "streaming") {
    metricsParts.push(gp && gp.total_bytes > 0 ? `${percent}% em cache` : `${percent}% enviado`);
    metricsParts.push(formatBps(bps));
  } else if (phase === "failed" && m?.error) {
    metricsParts.push(m.error);
  }
  const metricsText = metricsParts.join(" · ");
  const hudText = phase === "connecting"
    ? "Conectando P2P…"
    : `${remux ? "Streaming adaptado" : "Enviando P2P"} (${formatBps(bps)})`;
  return {
    active,
    phase,
    title,
    percent,
    metricsText,
    hudText,
    error: failed ? m.error : null,
    hiddenWarning: Boolean(m?.hiddenWarning),
  };
}

export function renderHostPrepare(view, appNotice, streamInfo, remuxNotice) {
  const inviteEl = $("host-invite");
  inviteEl.hidden = view.connected;
  const code = view.code || "";
  $("host-invite-code").textContent = code || "Gerando código…";
  $("copy-code").disabled = !code;
  $("copy-link").disabled = !code;
  $("host-invite-label").textContent = view.relay_offline
    ? "Sem conexão com o servidor. Tentando reconectar…"
    : view.peer_left
    ? `${view.peer || "A outra pessoa"} saiu da sala. Envie o código para reconectar.`
    : "Envie este código para quem vai assistir com você";

  $("stream").hidden = true;
  const hostFile = view.host_file_name;
  $("guest-file").textContent = hostFile || "Nenhum arquivo ainda";
  $("pick-text").textContent = hostFile ? "Trocar arquivo" : "Escolher filme";
  const pickLabel = $("pick-label");
  pickLabel.classList.toggle("primary", !view.matched);
  pickLabel.classList.toggle("secondary", view.matched);

  const selfBadge = $("guest-copy-badge");
  const peerBadge = $("host-copy-badge");
  selfBadge.className = "copy-badge muted";
  peerBadge.className = "copy-badge muted";

  if (view.host_media && view.matched) {
    selfBadge.textContent = "Conferido";
    selfBadge.classList.add("is-ok");
  } else if (view.mismatch) {
    selfBadge.textContent = "Conteúdo diferente";
    selfBadge.classList.add("is-warning");
  } else if (!$("hashing").hidden || view.host_loading) {
    selfBadge.textContent = "Conferindo…";
  } else if (view.host_media) {
    selfBadge.textContent = view.host_ready ? "Conferido" : (remuxNotice?.kind === "unsupported" ? "Incompatível" : "Preparando…");
    if (view.host_ready) selfBadge.classList.add("is-ok");
    if (remuxNotice?.kind === "unsupported") selfBadge.classList.add("is-warning");
  } else {
    selfBadge.textContent = "Falta escolher";
  }

  $("host-card-name").textContent = view.peer || "Convidado";
  if (!view.connected) {
    $("host-file").textContent = "Esperando alguém entrar na sala…";
    peerBadge.textContent = "Aguardando";
  } else if (streamInfo && streamInfo.active) {
    $("host-file").textContent = view.guest_file_name || hostFile || "Recebendo por streaming…";
    if (streamInfo.phase === "connecting") {
      peerBadge.textContent = "Conectando P2P…";
    } else {
      peerBadge.textContent = "Via streaming P2P";
      peerBadge.classList.add("is-ok");
    }
  } else {
    $("host-file").textContent = view.guest_file_name
      || (view.guest_loading ? "Conferindo o arquivo…" : "Esperando escolher o filme…");
    if (view.guest_media && view.matched) {
      peerBadge.textContent = view.guest_ready ? "Conferido" : "Preparando…";
      if (view.guest_ready) peerBadge.classList.add("is-ok");
    } else if (view.mismatch) {
      peerBadge.textContent = "Conteúdo diferente";
      peerBadge.classList.add("is-warning");
    } else if (view.guest_loading) {
      peerBadge.textContent = "Conferindo…";
    } else {
      peerBadge.textContent = "Falta escolher";
    }
  }

  $("format-warning").hidden = true;
  const selfFormatWarning = $("self-format-warning");
  if (remuxNotice && remuxNotice.text) {
    selfFormatWarning.hidden = false;
    selfFormatWarning.className = remuxNotice.kind === "ready" ? "muted small-text" : "warning small-text";
    selfFormatWarning.textContent = remuxNotice.text;
  } else {
    const risky = Boolean(hostFile && RISKY_FORMATS.test(hostFile));
    selfFormatWarning.hidden = !risky;
    selfFormatWarning.className = "warning small-text";
    selfFormatWarning.textContent = risky
      ? "Este formato costuma não tocar no navegador (MKV e áudio AC-3/DTS, por exemplo). Se não funcionar, use o app."
      : "";
  }

  const strip = $("host-stream-strip");
  if (streamInfo) {
    strip.hidden = false;
    strip.classList.toggle("is-warning", streamInfo.phase === "failed" || streamInfo.hiddenWarning);
    $("host-stream-title").textContent = streamInfo.title;
    $("host-stream-metrics").textContent = streamInfo.metricsText;
    $("host-stream-bar-wrap").hidden = streamInfo.phase === "failed";
    $("host-stream-bar").style.width = `${streamInfo.percent}%`;
    const tabNote = $("host-stream-tab-note");
    tabNote.className = streamInfo.hiddenWarning ? "small-text warning" : "small-text muted";
    tabNote.textContent = streamInfo.hiddenWarning
      ? "Esta aba ficou em segundo plano durante o streaming. Navegadores reduzem a atividade de abas ocultas; mantenha a aba visível para evitar travamentos."
      : "Mantenha esta aba aberta e visível enquanto o streaming estiver ativo.";
  } else {
    strip.hidden = true;
  }

  const prepareEmblem = $("prepare-emblem");
  if (!view.connected) {
    $("prepare-heading").textContent = "Aguardando convidado";
    setEmblem(prepareEmblem, "waiting", 18);
  } else if (view.matched) {
    $("prepare-heading").textContent = view.guest_streaming || streamInfo?.active
      ? "Streaming pronto para começar"
      : "Vocês têm o mesmo filme";
    setEmblem(prepareEmblem, "together", 18);
  } else if (view.mismatch) {
    $("prepare-heading").textContent = "As cópias são diferentes";
    setEmblem(prepareEmblem, "apart", 18);
  } else if (!view.host_media) {
    $("prepare-heading").textContent = "Escolha o filme";
    setEmblem(prepareEmblem, "waiting", 18);
  } else if (streamInfo?.active) {
    $("prepare-heading").textContent = `Enviando o filme para ${view.peer || "quem entrou"}`;
    setEmblem(prepareEmblem, "waiting", 18);
  } else {
    $("prepare-heading").textContent = `Esperando o filme de ${view.peer || "quem entrou"}`;
    setEmblem(prepareEmblem, "waiting", 18);
  }

  const copyStatus = $("copy-status");
  let copyText = "";
  if (view.matched) {
    copyText = view.guest_streaming || streamInfo?.active
      ? `${view.peer || "O convidado"} está recebendo o filme por streaming P2P.`
      : "Os dois arquivos conferem.";
  } else if (view.mismatch) {
    copyText = "Os arquivos têm conteúdo diferente. Escolham a mesma cópia.";
  }
  copyStatus.hidden = !copyText;
  copyStatus.textContent = copyText;

  const startWrap = $("host-start-wrap");
  const startBtn = $("start-watching");
  const showStart = view.connected && (view.matched || view.preparing || view.scheduled);
  startWrap.hidden = !showStart;
  if (showStart) {
    if ((view.preparing || view.scheduled) && view.starting) {
      startBtn.disabled = false;
      startBtn.textContent = "Cancelar início";
    } else if (view.preparing || view.scheduled) {
      startBtn.disabled = true;
      startBtn.textContent = "Preparando…";
    } else if (view.ready) {
      startBtn.disabled = false;
      startBtn.textContent = "Começar a assistir";
    } else {
      startBtn.disabled = true;
      startBtn.textContent = !view.clock_ready ? "Acertando os relógios…" : "Preparando…";
    }
  }

  const prepNotice = appNotice || view.notice || (view.relay_offline && view.connected ? "Sem conexão com o servidor. Tentando reconectar…" : "");
  $("prepare-notice").hidden = !prepNotice;
  $("prepare-notice").textContent = prepNotice || "";
}

export function hostStatusText(view) {
  if (!view.connected) {
    return view.relay_offline
      ? "Sem conexão com o servidor. Tentando reconectar…"
      : "Sala aberta. Aguardando alguém entrar.";
  }
  if (!view.host_media) {
    return "Escolha sua cópia do filme.";
  }
  if (!view.matched) {
    return view.mismatch
      ? "As cópias são diferentes."
      : "Esperando o convidado escolher a cópia do filme.";
  }
  if (view.buffering || view.guest_buffering || view.waiting_buffer) {
    return "Carregando…";
  }
  if (view.preparing) {
    return "Preparando…";
  }
  if (view.scheduled) {
    return "Começando…";
  }
  if (view.playing) {
    return view.drift_ms !== null
      ? `Sincronizado (diferença de ${Math.abs(view.drift_ms).toFixed(0)} ms).`
      : "Reproduzindo em sincronia.";
  }
  if (!view.clock_ready) {
    return "Acertando os relógios…";
  }
  if (!view.guest_ready) {
    return "Esperando o convidado ficar pronto.";
  }
  if (!view.host_ready) {
    return "Carregando…";
  }
  return "Pronto. Qualquer um pode apertar reproduzir.";
}
