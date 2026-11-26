import init, { WebGuest } from "./pkg/sync2gether_web.js";
import { Player, now } from "./player.js";
import { RELAY_URL } from "./config.js";
import {
  ICON_ALERT,
  ICON_CANCEL,
  ICON_INFO,
  ICON_PAUSE,
  ICON_PLAY,
  ICON_VOL_MUTED,
  ICON_VOL_ON,
  centerLoadingState,
  centerNoticeState,
  formatTime,
  mediaTitle,
  peerPresence,
  setEmblem,
  videoBannerState,
} from "./ui.js";

const $ = (id) => document.getElementById(id);
const STORE = "sync2gether.room";
const RISKY_FORMATS = /\.(mkv|avi|wmv|flv|ts|m2ts|vob|rmvb)$/i;
const CONTROLS_HIDE_MS = 3000;
const TOAST_MS = 4000;

const app = {
  guest: null,
  code: null,
  socket: null,
  connectAt: null,
  view: null,
  everConnected: false,
  hashJob: 0,
  objectUrl: null,
  notice: null,
  toast: null,
  toastUntil: 0,
  dragging: false,
  lastRender: 0,
  stream: null,
  lastActivity: 0,
  controlsVisible: true,
  popoverOpen: false,
  forcePrepare: false,
  unmutedVolume: 1,
  prevPeer: null,
  prevConnected: false,
  prevPaused: null,
  prevPausedBy: null,
  prevSync: false,
};

const player = new Player($("video"), {
  prepared: (token) => call((g) => g.player_prepared(token, now())),
  started: (token, at) => call((g) => g.player_started(token, at, now())),
  missed: (token) => call((g) => g.player_missed(token, now())),
  error: (message) => call((g) => g.player_error(message, now())),
  unlock: () => { $("unlock").hidden = false; },
  muted: () => { $("unmute").hidden = false; },
  notice: (message) => { app.notice = message; render(true); },
});

function call(action) {
  if (!app.guest) return;
  action(app.guest);
  pump();
}

function relayUrl() {
  if (RELAY_URL) return RELAY_URL;
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${location.host}/relay`;
}

function browserName() {
  const ua = navigator.userAgent;
  const name = /Edg\//.test(ua) ? "Edge"
    : /Firefox\//.test(ua) ? "Firefox"
    : /Chrome\//.test(ua) ? "Chrome"
    : /Safari\//.test(ua) ? "Safari"
    : "navegador";
  const mobile = /Mobi|Android|iPhone|iPad/.test(ua) ? ", celular" : "";
  return `Navegador (${name}${mobile})`;
}

function hex(bytes) {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
function unhex(text) {
  const pairs = text.match(/[0-9a-f]{2}/gi) || [];
  return new Uint8Array(pairs.map((p) => parseInt(p, 16)));
}

function remember() {
  const key = app.guest?.resume_key();
  if (!key) return;
  try {
    sessionStorage.setItem(STORE, JSON.stringify({ code: app.code, key: hex(key) }));
  } catch {}
}
function forget() {
  try { sessionStorage.removeItem(STORE); } catch {}
}
function remembered() {
  try {
    return JSON.parse(sessionStorage.getItem(STORE) || "null");
  } catch {
    return null;
  }
}

function setToast(message) {
  app.toast = message;
  app.toastUntil = now() + TOAST_MS;
}

function pump() {
  const guest = app.guest;
  if (!guest) return;
  const output = guest.take_output();
  if (output.length && app.socket && app.socket.readyState === WebSocket.OPEN) {
    app.socket.send(output);
  }
  for (const command of JSON.parse(guest.take_commands())) {
    if (command.kind === "connect") {
      app.connectAt = command.at;
    } else if (command.kind === "close") {
      closeSocket();
    } else if (command.kind.startsWith("stream_")) {
      app.stream?.command(command);
    } else {
      player.run(command);
    }
  }
  app.stream?.pump();
  render(false);
}

function connect() {
  const socket = new WebSocket(relayUrl());
  socket.binaryType = "arraybuffer";
  app.socket = socket;
  socket.onopen = () => {
    if (app.socket !== socket) return;
    call((g) => g.socket_open(now()));
  };
  socket.onmessage = (event) => {
    if (app.socket !== socket || !(event.data instanceof ArrayBuffer)) return;
    call((g) => g.socket_data(new Uint8Array(event.data), now()));
  };
  const closed = () => {
    if (app.socket !== socket) return;
    app.socket = null;
    call((g) => g.socket_closed(now()));
  };
  socket.onclose = closed;
  socket.onerror = closed;
}

function closeSocket() {
  const socket = app.socket;
  app.socket = null;
  if (socket) {
    socket.onclose = null;
    socket.onerror = null;
    socket.onmessage = null;
    try { socket.close(); } catch {}
  }
}

function tick() {
  const guest = app.guest;
  if (!guest) return;
  if (app.connectAt !== null && !app.socket && now() >= app.connectAt) {
    app.connectAt = null;
    connect();
  }
  guest.player_status(JSON.stringify(player.status()), now());
  guest.tick(now());
  pump();
}

function join(code) {
  let guest;
  try {
    guest = new WebGuest(code, browserName());
  } catch (error) {
    showEntry(String(error && error.message || error));
    return;
  }
  app.guest = guest;
  app.code = code;
  app.forcePrepare = false;
  app.popoverOpen = false;
  app.prevPeer = null;
  app.prevConnected = false;
  app.prevPaused = null;
  app.prevPausedBy = null;
  app.prevSync = false;
  const saved = remembered();
  if (saved && saved.code === code && saved.key) {
    guest.resume_with(unhex(saved.key));
  }
  guest.start(now());
  pump();
}

function leave() {
  call((g) => g.leave(now()));
  forget();
  app.stream?.stop();
  app.stream = null;
  setPlayerMode(false);
  showEnded("Você saiu da sala.", "");
}

function chooseFile(file) {
  if (!file || !app.guest) return;
  app.stream?.stop();
  app.stream = null;
  app.forcePrepare = false;
  const id = ++app.hashJob;
  app.guest.set_loading(file.name, 0, file.size);
  $("hashing").hidden = false;
  $("hashing-bar").style.width = "0%";
  const worker = new Worker(new URL("./hash-worker.js", import.meta.url), { type: "module" });
  worker.onmessage = (event) => {
    const data = event.data;
    if (data.id !== id || id !== app.hashJob) {
      worker.terminate();
      return;
    }
    if (data.progress !== undefined) {
      $("hashing-bar").style.width = `${(100 * data.progress / Math.max(1, data.total)).toFixed(1)}%`;
      call((g) => g.set_loading(file.name, data.progress, data.total));
      return;
    }
    worker.terminate();
    $("hashing").hidden = true;
    if (data.error) {
      app.notice = "Não foi possível ler o arquivo.";
      call((g) => g.set_media(undefined, 0, undefined));
      return;
    }
    if (app.objectUrl) URL.revokeObjectURL(app.objectUrl);
    app.objectUrl = URL.createObjectURL(file);
    app.notice = null;
    const generation = app.guest.set_media(data.media.hash, data.media.bytes, file.name);
    player.load(app.objectUrl, generation);
    pump();
  };
  worker.postMessage({ id, file });
}

export function startStreaming(createStream) {
  if (!app.guest) return;
  app.hashJob++;
  app.forcePrepare = false;
  $("hashing").hidden = true;
  app.stream?.stop();
  app.stream = createStream({ guest: app.guest, player, pump, now });
}

function setPlayerMode(active) {
  document.body.classList.toggle("in-player", active);
  if (!active) {
    document.body.classList.remove("controls-hidden");
    app.popoverOpen = false;
    $("room-popover").hidden = true;
    $("presence-btn").setAttribute("aria-expanded", "false");
  }
}

export function show(id) {
  for (const section of ["entry", "connecting", "ended", "room"]) {
    $(section).hidden = section !== id;
  }
  const inRoom = id === "room";
  $("brand").hidden = inRoom;
  $("leave").hidden = !inRoom;
  if (!inRoom) {
    $("room-code").hidden = true;
    $("stage-title").hidden = true;
    $("presence-btn").hidden = true;
    setPlayerMode(false);
  }
}

export function showEntry(error) {
  show("entry");
  $("entry-error").hidden = !error;
  $("entry-error").textContent = error || "";
}

export function showEnded(title, text) {
  show("ended");
  $("ended-title").textContent = title;
  $("ended-text").textContent = text;
}

function noteActivity() {
  app.lastActivity = now();
  if (!app.controlsVisible) {
    app.controlsVisible = true;
    document.body.classList.remove("controls-hidden");
  }
}

function updateControlsVisibility(view) {
  if (!document.body.classList.contains("in-player")) return;
  const keep = app.popoverOpen
    || app.dragging
    || !view.playing
    || !view.connected
    || view.buffering
    || view.host_buffering
    || view.waiting_buffer
    || view.preparing
    || view.scheduled
    || Boolean(view.error);
  const visible = keep || (now() - app.lastActivity < CONTROLS_HIDE_MS);
  app.controlsVisible = visible;
  document.body.classList.toggle("controls-hidden", !visible);
}

function trackRoomEvents(view) {
  const peer = view.peer || "O anfitrião";
  if (view.connected && !app.prevConnected) {
    if (app.prevPeer !== null) {
      setToast(`Reconectado à sala de ${peer}.`);
    } else if (view.peer) {
      setToast(`Você entrou na sala de ${view.peer}.`);
    }
  } else if (!view.connected && app.prevConnected && !view.peer_left) {
    setToast(`Conexão com ${peer} perdida. Reconectando…`);
  }
  const inSync = view.connected && view.matched && view.playing && view.drift_ms !== null;
  if (inSync && !app.prevSync) {
    setToast("Em sincronia.");
  }
  if (view.peer) app.prevPeer = view.peer;
  app.prevConnected = view.connected;
  app.prevPaused = view.paused;
  app.prevPausedBy = view.paused_by;
  app.prevSync = inSync;
}

function render(force) {
  const guest = app.guest;
  if (!guest) return;
  const time = now();
  if (!force && time - app.lastRender < 120) return;
  app.lastRender = time;
  const view = JSON.parse(guest.view());
  app.view = view;
  if (view.connected && !app.everConnected) {
    app.everConnected = true;
  }
  if (view.connected) remember();
  if (view.ended) {
    forget();
    closeSocket();
    if (!$("ended").hidden) return;
    if (view.error) {
      showEnded("Não foi possível entrar", view.error);
    } else {
      showEnded("Sala encerrada", view.notice || "");
    }
    player.unload(0);
    return;
  }
  if (!app.everConnected) {
    show("connecting");
    $("connecting-text").textContent = "Conectando à sala…";
    return;
  }
  trackRoomEvents(view);
  renderRoom(view);
}

function renderRoom(view) {
  show("room");
  if (app.toast && now() > app.toastUntil) {
    app.toast = null;
  }

  const inPlayer = (view.matched || Boolean(app.stream?.started))
    && !app.forcePrepare;
  setPlayerMode(inPlayer);

  $("prepare").hidden = inPlayer;
  $("stage").hidden = !inPlayer;
  $("room-code").hidden = inPlayer || !view.code;
  $("room-code").textContent = view.code || "";
  $("stage-title").hidden = !inPlayer;
  $("stage-title").textContent = mediaTitle(view.host_file_name);
  $("presence-btn").hidden = !inPlayer;

  const hostFile = view.host_file_name;
  $("host-card-name").textContent = view.peer || "Anfitrião";
  $("host-file").textContent = hostFile || (view.host_loading ? "O anfitrião está abrindo o filme…" : "Esperando o anfitrião escolher o filme…");
  const risky = hostFile && RISKY_FORMATS.test(hostFile);
  $("format-warning").hidden = !risky;
  $("format-warning").textContent = risky
    ? "Este formato costuma não tocar no navegador (MKV e áudio AC-3/DTS, por exemplo). Se não funcionar, use o app."
    : "";
  $("stream").hidden = !(view.host_web_stream && view.host_media);

  const guestFile = view.guest_file_name || (app.stream ? hostFile : null);
  $("guest-file").textContent = guestFile || "Nenhum arquivo ainda";
  $("pick-text").textContent = guestFile ? "Trocar arquivo" : "Usar minha cópia";

  const guestBadge = $("guest-copy-badge");
  const hostBadge = $("host-copy-badge");
  guestBadge.className = "copy-badge muted";
  hostBadge.className = "copy-badge muted";
  if (app.stream) {
    guestBadge.textContent = "Via streaming P2P";
    guestBadge.classList.add("is-ok");
  } else if (view.guest_media && view.matched) {
    guestBadge.textContent = "Conferido";
    guestBadge.classList.add("is-ok");
  } else if (view.mismatch) {
    guestBadge.textContent = "Conteúdo diferente";
    guestBadge.classList.add("is-warning");
  } else if (!$("hashing").hidden) {
    guestBadge.textContent = "Conferindo…";
  } else {
    guestBadge.textContent = "Falta escolher";
  }

  if (view.host_media) {
    hostBadge.textContent = view.host_ready ? "Conferido" : "Preparando…";
    if (view.host_ready) hostBadge.classList.add("is-ok");
  } else if (view.host_loading) {
    hostBadge.textContent = "Conferindo…";
  } else {
    hostBadge.textContent = "Aguardando";
  }

  const prepareEmblem = $("prepare-emblem");
  if (view.matched) {
    $("prepare-heading").textContent = "Vocês têm o mesmo filme";
    setEmblem(prepareEmblem, "together", 18);
  } else if (view.mismatch) {
    $("prepare-heading").textContent = "As cópias são diferentes";
    setEmblem(prepareEmblem, "apart", 18);
  } else if (!view.host_media) {
    $("prepare-heading").textContent = "Esperando o filme do anfitrião";
    setEmblem(prepareEmblem, "waiting", 18);
  } else {
    $("prepare-heading").textContent = "Escolha o filme";
    setEmblem(prepareEmblem, "waiting", 18);
  }

  const copyStatus = $("copy-status");
  let copyText = "";
  if (app.stream) {
    copyText = app.stream.describe();
  } else if (view.guest_media && view.matched) {
    copyText = "Sua cópia confere com a do anfitrião.";
  } else if (view.mismatch) {
    copyText = "Sua cópia é diferente da do anfitrião. Escolha o mesmo arquivo.";
  }
  copyStatus.hidden = !copyText;
  copyStatus.textContent = copyText;

  const prepNotice = app.notice || view.notice;
  $("prepare-notice").hidden = !prepNotice;
  $("prepare-notice").textContent = prepNotice || "";

  let status = "";
  if (!view.connected) {
    status = "Reconectando…";
  } else if (!view.host_media) {
    status = "Esperando o anfitrião.";
  } else if (!view.matched) {
    status = "Escolha sua cópia do filme ou assista por streaming.";
  } else if (view.buffering || view.host_buffering) {
    status = "Carregando…";
  } else if (view.playing && view.drift_ms !== null) {
    status = `Sincronizado (diferença de ${Math.abs(view.drift_ms).toFixed(0)} ms).`;
  } else if (view.scheduled_start_ms !== null && !view.playing) {
    status = "Começando…";
  } else if (!view.clock_ready) {
    status = "Acertando os relógios…";
  } else if (!view.host_ready) {
    status = "Esperando o anfitrião ficar pronto.";
  } else {
    status = "Pronto. Qualquer um pode apertar reproduzir.";
  }
  $("status").textContent = status;
  $("popover-code").textContent = view.code ? `Código: ${view.code}` : "";
  const metrics = [];
  if (view.clock_ready && Number.isFinite(view.rtt_ms)) {
    metrics.push(`Ida e volta: ${Math.round(view.rtt_ms)} ms`);
  }
  if (view.drift_ms !== null && Number.isFinite(view.drift_ms)) {
    metrics.push(`Diferença: ${Math.abs(view.drift_ms).toFixed(0)} ms`);
  }
  $("popover-metrics").textContent = metrics.join(" · ");

  const presence = peerPresence(view, app.stream);
  $("peer").textContent = presence.name;
  $("peer-detail").textContent = presence.detail;
  $("peer-detail").classList.toggle("is-warning", presence.warning);
  setEmblem($("presence-emblem"), presence.state, 14);

  const loading = centerLoadingState(view, app.stream);
  const loadingEl = $("center-loading");
  loadingEl.hidden = !loading;
  if (loading) {
    $("center-loading-title").textContent = loading.title;
    $("center-loading-icon").hidden = !loading.spinner;
    $("center-loading-detail").hidden = !loading.detail;
    $("center-loading-detail").textContent = loading.detail || "";
  }

  const center = centerNoticeState(view, Boolean(loading));
  const centerEl = $("center-notice");
  centerEl.hidden = !center;
  $("stage-dim").dataset.dim = center ? center.dim : "";
  if (center) {
    setEmblem($("center-emblem"), center.state, center.diameter);
    $("center-notice-title").textContent = center.title;
    $("center-notice-detail").textContent = center.detail;
  }

  const banner = videoBannerState(view, app.notice, app.toast);
  const bannerEl = $("video-notice");
  bannerEl.hidden = !banner;
  if (banner) {
    bannerEl.dataset.tone = banner.kind === "alert" ? "warning" : "info";
    $("notice").textContent = banner.text;
    const iconEl = $("video-notice-icon");
    if (banner.kind === "spinner") {
      iconEl.innerHTML = `<span class="spinner small-spin"></span>`;
    } else if (banner.kind === "alert") {
      iconEl.innerHTML = ICON_ALERT;
    } else {
      iconEl.innerHTML = ICON_INFO;
    }
  } else {
    $("notice").textContent = "";
  }

  const streamHud = $("stream-hud");
  if (app.stream && app.stream.phase !== "failed") {
    streamHud.hidden = false;
    streamHud.textContent = app.stream.describe();
    streamHud.classList.toggle("is-warning", app.stream.phase !== "streaming");
  } else {
    streamHud.hidden = true;
  }

  const cancelling = view.preparing || view.scheduled || view.waiting_buffer;
  const toggle = $("toggle");
  const toggleIcon = $("toggle-icon");
  const canControl = view.connected && view.matched;
  toggle.disabled = !canControl;
  $("back").disabled = !canControl;
  $("forward").disabled = !canControl;
  $("seekbar").disabled = !canControl;

  if (cancelling) {
    toggleIcon.innerHTML = ICON_CANCEL;
    toggle.setAttribute("aria-label", "Cancelar operação");
    toggle.title = "Cancelar operação";
  } else if (view.playing) {
    toggleIcon.innerHTML = ICON_PAUSE;
    toggle.setAttribute("aria-label", "Pausar");
    toggle.title = "Pausar · Espaço";
  } else {
    toggleIcon.innerHTML = ICON_PLAY;
    toggle.setAttribute("aria-label", "Reproduzir");
    toggle.title = "Reproduzir · Espaço";
  }

  const duration = view.host_duration || player.status().duration;
  const currentPos = app.dragging && duration > 0
    ? duration * Number($("seekbar").value) / 1000
    : view.position;
  const fraction = duration > 0 ? Math.max(0, Math.min(1, currentPos / duration)) : 0;
  if (!app.dragging) {
    $("seekbar").value = String(Math.round(1000 * fraction));
  }
  const pct = `${(fraction * 100).toFixed(3)}%`;
  $("seek-fill").style.width = pct;
  $("seek-thumb").style.left = pct;

  const bufFrac = duration > 0 ? Math.max(0, Math.min(1, (view.buffered_until || 0) / duration)) : 0;
  $("seek-buffered").style.width = `${(bufFrac * 100).toFixed(3)}%`;

  const peerDot = $("seek-peer");
  if (view.connected && view.host_position !== null && view.host_position !== undefined && duration > 0) {
    const peerFrac = Math.max(0, Math.min(1, view.host_position / duration));
    peerDot.hidden = false;
    peerDot.style.left = `${(peerFrac * 100).toFixed(3)}%`;
  } else {
    peerDot.hidden = true;
  }

  $("time").textContent = formatTime(currentPos);
  $("duration").textContent = formatTime(duration);
  syncVolumeIcon();
  updateControlsVisibility(view);
}

function syncVolumeIcon() {
  const video = $("video");
  const muted = video.muted || video.volume === 0;
  $("mute-icon").innerHTML = muted ? ICON_VOL_MUTED : ICON_VOL_ON;
  $("mute").setAttribute("aria-label", muted ? "Ativar som" : "Silenciar");
  $("volume").value = muted ? "0" : String(video.volume);
}

function toggleMute() {
  const video = $("video");
  if (video.muted || video.volume === 0) {
    video.muted = false;
    video.volume = app.unmutedVolume > 0 ? app.unmutedVolume : 1;
    $("unmute").hidden = true;
  } else {
    app.unmutedVolume = video.volume;
    video.muted = true;
  }
  syncVolumeIcon();
}

function togglePlay() {
  if (!app.view || !app.view.connected || !app.view.matched) return;
  noteActivity();
  player.unlock();
  const cancelling = app.view.preparing || app.view.scheduled || app.view.waiting_buffer;
  call((g) => (app.view.playing || cancelling ? g.pause(now()) : g.play(now())));
}

function seekBy(delta) {
  if (!app.view || !app.view.connected || !app.view.matched) return;
  const duration = app.view.host_duration || player.status().duration;
  if (duration <= 0) return;
  noteActivity();
  const target = Math.max(0, Math.min(duration, app.view.position + delta));
  call((g) => g.seek(target, now()));
}

function toggleFullscreen() {
  const stage = $("stage");
  if (document.fullscreenElement) {
    document.exitFullscreen().catch(() => {});
  } else {
    (stage.requestFullscreen?.() || $("video").webkitEnterFullscreen?.());
  }
}

export function bindEvents() {
  $("entry-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const value = $("entry-code").value.trim();
    const code = value.includes("#") ? value.slice(value.indexOf("#") + 1) : value;
    player.unlock();
    show("connecting");
    join(decodeURIComponent(code));
  });
  $("again").addEventListener("click", () => {
    app.guest = null;
    app.everConnected = false;
    app.notice = null;
    app.toast = null;
    $("entry-code").value = "";
    showEntry("");
  });
  $("leave").addEventListener("click", leave);
  $("pick").addEventListener("click", () => player.unlock());
  $("pick").addEventListener("change", (event) => chooseFile(event.target.files[0]));
  $("stream").addEventListener("click", async () => {
    player.unlock();
    const module = await import("./stream.js");
    startStreaming(module.createStream);
  });
  $("toggle").addEventListener("click", togglePlay);
  $("back").addEventListener("click", () => seekBy(-10));
  $("forward").addEventListener("click", () => seekBy(10));
  $("video").addEventListener("click", () => {
    if (app.popoverOpen) {
      app.popoverOpen = false;
      $("room-popover").hidden = true;
      $("presence-btn").setAttribute("aria-expanded", "false");
      return;
    }
    togglePlay();
  });

  const seekWrap = $("seek-wrap");
  const seekbar = $("seekbar");
  const seekTip = $("seek-tip");
  seekbar.addEventListener("input", () => {
    app.dragging = true;
    seekWrap.classList.add("is-dragging");
    noteActivity();
    if (app.view) renderRoom(app.view);
  });
  seekbar.addEventListener("change", () => {
    app.dragging = false;
    seekWrap.classList.remove("is-dragging");
    const duration = app.view?.host_duration || player.status().duration || 0;
    if (duration > 0) call((g) => g.seek(duration * seekbar.value / 1000, now()));
  });
  seekWrap.addEventListener("pointermove", (event) => {
    const duration = app.view?.host_duration || player.status().duration || 0;
    if (duration <= 0 || seekbar.disabled) {
      seekTip.hidden = true;
      return;
    }
    const rect = seekWrap.getBoundingClientRect();
    const frac = Math.max(0, Math.min(1, (event.clientX - rect.left) / Math.max(1, rect.width)));
    seekTip.hidden = false;
    seekTip.textContent = formatTime(frac * duration);
    seekTip.style.left = `${(frac * 100).toFixed(2)}%`;
  });
  seekWrap.addEventListener("pointerleave", () => { seekTip.hidden = true; });

  $("mute").addEventListener("click", () => {
    noteActivity();
    toggleMute();
  });
  $("volume").addEventListener("input", (event) => {
    noteActivity();
    const value = Number(event.target.value);
    $("video").volume = value;
    $("video").muted = value === 0;
    if (value > 0) {
      app.unmutedVolume = value;
      $("unmute").hidden = true;
    }
    syncVolumeIcon();
  });
  $("fullscreen").addEventListener("click", () => {
    noteActivity();
    toggleFullscreen();
  });
  $("unlock").addEventListener("click", () => {
    $("unlock").hidden = true;
    player.unlock();
  });
  $("unmute").addEventListener("click", () => {
    $("unmute").hidden = true;
    $("video").muted = false;
    syncVolumeIcon();
  });

  $("presence-btn").addEventListener("click", () => {
    noteActivity();
    app.popoverOpen = !app.popoverOpen;
    $("room-popover").hidden = !app.popoverOpen;
    $("presence-btn").setAttribute("aria-expanded", String(app.popoverOpen));
  });
  $("popover-close").addEventListener("click", () => {
    app.popoverOpen = false;
    $("room-popover").hidden = true;
    $("presence-btn").setAttribute("aria-expanded", "false");
  });
  $("change-file").addEventListener("click", () => {
    app.popoverOpen = false;
    $("room-popover").hidden = true;
    $("presence-btn").setAttribute("aria-expanded", "false");
    app.forcePrepare = true;
    if (app.view) renderRoom(app.view);
  });

  $("stage").addEventListener("pointermove", noteActivity);
  $("stage").addEventListener("pointerdown", noteActivity);

  window.addEventListener("keydown", (event) => {
    if (!document.body.classList.contains("in-player")) return;
    if (event.target instanceof HTMLInputElement && event.target.type === "text") return;
    if (event.code === "Space") {
      event.preventDefault();
      togglePlay();
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      seekBy(-10);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      seekBy(10);
    } else if (event.key === "f" || event.key === "F") {
      event.preventDefault();
      toggleFullscreen();
    } else if (event.key === "m" || event.key === "M") {
      event.preventDefault();
      noteActivity();
      toggleMute();
    } else if (event.key === "Escape" && app.popoverOpen) {
      app.popoverOpen = false;
      $("room-popover").hidden = true;
      $("presence-btn").setAttribute("aria-expanded", "false");
    }
  });

  window.addEventListener("pagehide", () => {
    if (app.guest && app.socket) {
      app.guest.leave(now());
      const output = app.guest.take_output();
      if (output.length && app.socket.readyState === WebSocket.OPEN) app.socket.send(output);
    }
  });
}

export function showRoom(view) {
  app.everConnected = true;
  app.view = view;
  noteActivity();
  renderRoom(view);
}

function boot() {
  bindEvents();

  setInterval(tick, 20);
  try {
    const ticker = new Worker(new URL("./ticker.js", import.meta.url));
    ticker.onmessage = tick;
  } catch {}

  const fragment = decodeURIComponent(location.hash.replace(/^#/, ""));
  if (fragment) {
    history.replaceState(null, "", location.pathname + location.search);
    show("connecting");
    join(fragment);
    return;
  }
  const saved = remembered();
  if (saved && saved.code) {
    show("connecting");
    join(saved.code);
    return;
  }
  showEntry("");
}

export function start() {
  init().then(boot, () => {
    showEntry("Este navegador não conseguiu carregar o sync2gether. Atualize o navegador e tente de novo.");
  });
}
