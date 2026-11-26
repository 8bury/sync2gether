import init, { WebGuest } from "./pkg/sync2gether_web.js";
import { Player, now } from "./player.js";
import { RELAY_URL } from "./config.js";

const $ = (id) => document.getElementById(id);
const STORE = "sync2gether.room";
const RISKY_FORMATS = /\.(mkv|avi|wmv|flv|ts|m2ts|vob|rmvb)$/i;

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
  dragging: false,
  lastRender: 0,
  stream: null,
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
  showEnded("Você saiu da sala.", "");
}

function chooseFile(file) {
  if (!file || !app.guest) return;
  app.stream?.stop();
  app.stream = null;
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
  $("hashing").hidden = true;
  app.stream?.stop();
  app.stream = createStream({ guest: app.guest, player, pump, now });
}

function format(seconds) {
  if (!Number.isFinite(seconds) || seconds < 0) seconds = 0;
  const s = Math.floor(seconds % 60).toString().padStart(2, "0");
  const m = Math.floor(seconds / 60) % 60;
  const h = Math.floor(seconds / 3600);
  return h > 0 ? `${h}:${m.toString().padStart(2, "0")}:${s}` : `${m}:${s}`;
}

function show(id) {
  for (const section of ["entry", "connecting", "ended", "room"]) {
    $(section).hidden = section !== id;
  }
}

function showEntry(error) {
  show("entry");
  $("peer").hidden = true;
  $("leave").hidden = true;
  $("entry-error").hidden = !error;
  $("entry-error").textContent = error || "";
}

function showEnded(title, text) {
  show("ended");
  $("ended-title").textContent = title;
  $("ended-text").textContent = text;
  $("peer").hidden = true;
  $("leave").hidden = true;
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
  show("room");
  $("leave").hidden = false;
  $("peer").hidden = !view.peer;
  $("peer").textContent = view.peer ? `Na sala de ${view.peer}` : "";

  const hostFile = view.host_file_name;
  $("host-file").textContent = hostFile || (view.host_loading ? "O anfitrião está abrindo o filme…" : "Esperando o anfitrião escolher o filme…");
  const risky = hostFile && RISKY_FORMATS.test(hostFile);
  $("format-warning").hidden = !risky;
  $("format-warning").textContent = risky
    ? "Este formato costuma não tocar no navegador (MKV e áudio AC-3/DTS, por exemplo). Se não funcionar, use o app."
    : "";
  $("stream").hidden = !(view.host_web_stream && view.host_media);

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

  let status = "";
  if (!view.connected) {
    status = "Reconectando…";
  } else if (!view.host_media) {
    status = "Esperando o anfitrião.";
  } else if (!view.matched) {
    status = "Escolha sua cópia do filme ou assista por streaming.";
  } else if (view.buffering) {
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

  const notice = app.notice || view.notice;
  $("notice").hidden = !notice;
  $("notice").textContent = notice || "";

  const toggle = $("toggle");
  toggle.textContent = view.playing ? "❚❚" : "▶";
  toggle.setAttribute("aria-label", view.playing ? "Pausar" : "Reproduzir");
  toggle.disabled = !view.connected || !view.matched;
  $("seekbar").disabled = !view.connected || !view.matched;
  const duration = view.host_duration || player.status().duration;
  if (!app.dragging) {
    $("seekbar").value = duration > 0 ? Math.round(1000 * Math.min(1, view.position / duration)) : 0;
  }
  $("time").textContent = format(view.position);
  $("duration").textContent = format(duration);
}

function boot() {
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
  $("toggle").addEventListener("click", () => {
    player.unlock();
    call((g) => (app.view?.playing ? g.pause(now()) : g.play(now())));
  });
  const seekbar = $("seekbar");
  seekbar.addEventListener("input", () => { app.dragging = true; });
  seekbar.addEventListener("change", () => {
    app.dragging = false;
    const duration = app.view?.host_duration || 0;
    if (duration > 0) call((g) => g.seek(duration * seekbar.value / 1000, now()));
  });
  $("volume").addEventListener("input", (event) => {
    $("video").volume = Number(event.target.value);
    $("video").muted = false;
  });
  $("fullscreen").addEventListener("click", () => {
    const stage = $("stage");
    if (document.fullscreenElement) {
      document.exitFullscreen().catch(() => {});
    } else {
      (stage.requestFullscreen?.() || $("video").webkitEnterFullscreen?.());
    }
  });
  $("unlock").addEventListener("click", () => {
    $("unlock").hidden = true;
    player.unlock();
  });
  $("unmute").addEventListener("click", () => {
    $("unmute").hidden = true;
    $("video").muted = false;
  });
  window.addEventListener("pagehide", () => {
    if (app.guest && app.socket) {
      app.guest.leave(now());
      const output = app.guest.take_output();
      if (output.length && app.socket.readyState === WebSocket.OPEN) app.socket.send(output);
    }
  });

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

init().then(boot, () => {
  showEntry("Este navegador não conseguiu carregar o sync2gether. Atualize o navegador e tente de novo.");
});
