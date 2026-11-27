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
  constructor({ player, relayUrl, onView }) {
    this.player = player;
    this.relayUrl = relayUrl;
    this.onView = onView;
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

export function renderHostPrepare(view, appNotice) {
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
    selfBadge.textContent = view.host_ready ? "Conferido" : "Preparando…";
    if (view.host_ready) selfBadge.classList.add("is-ok");
  } else {
    selfBadge.textContent = "Falta escolher";
  }

  $("host-card-name").textContent = view.peer || "Convidado";
  if (!view.connected) {
    $("host-file").textContent = "Esperando alguém entrar na sala…";
    peerBadge.textContent = "Aguardando";
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

  const risky = Boolean(hostFile && RISKY_FORMATS.test(hostFile));
  const formatWarning = $("format-warning");
  formatWarning.hidden = !risky;
  formatWarning.className = "warning small-text";
  formatWarning.textContent = risky
    ? "Este formato costuma não tocar no navegador (MKV e áudio AC-3/DTS, por exemplo). Se não funcionar, use o app."
    : "";

  const prepareEmblem = $("prepare-emblem");
  if (!view.connected) {
    $("prepare-heading").textContent = "Aguardando convidado";
    setEmblem(prepareEmblem, "waiting", 18);
  } else if (view.matched) {
    $("prepare-heading").textContent = "Vocês têm o mesmo filme";
    setEmblem(prepareEmblem, "together", 18);
  } else if (view.mismatch) {
    $("prepare-heading").textContent = "As cópias são diferentes";
    setEmblem(prepareEmblem, "apart", 18);
  } else if (!view.host_media) {
    $("prepare-heading").textContent = "Escolha o filme";
    setEmblem(prepareEmblem, "waiting", 18);
  } else {
    $("prepare-heading").textContent = `Esperando o filme de ${view.peer || "quem entrou"}`;
    setEmblem(prepareEmblem, "waiting", 18);
  }

  const copyStatus = $("copy-status");
  let copyText = "";
  if (view.matched) {
    copyText = "Os dois arquivos conferem.";
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
