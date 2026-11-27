const SVG_NS = "http://www.w3.org/2000/svg";

export const ICON_PLAY = `<polygon points="7,4 20,12 7,20" fill="currentColor"/>`;
export const ICON_PAUSE = `<rect x="6" y="4.5" width="4.5" height="15" rx="1" fill="currentColor"/><rect x="13.5" y="4.5" width="4.5" height="15" rx="1" fill="currentColor"/>`;
export const ICON_CANCEL = `<path d="M6 6l12 12M18 6L6 18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>`;
export const ICON_VOL_ON = `<polygon points="4,9.5 8,9.5 13,5 13,19 8,14.5 4,14.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M16 8.5a5 5 0 0 1 0 7M18.8 6a8.5 8.5 0 0 1 0 12" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>`;
export const ICON_VOL_MUTED = `<polygon points="4,9.5 8,9.5 13,5 13,19 8,14.5 4,14.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M16 9l5 6M21 9l-5 6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>`;
export const ICON_INFO = `<svg viewBox="0 0 16 16" width="15" height="15"><circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M8 7v4M8 4.8v.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`;
export const ICON_ALERT = `<svg viewBox="0 0 16 16" width="15" height="15"><path d="M8 2.2L14.2 13H1.8L8 2.2z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/><path d="M8 6.2v3.2M8 11.4v.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`;

export function formatTime(seconds) {
  if (!Number.isFinite(seconds) || seconds < 0) seconds = 0;
  const s = Math.floor(seconds % 60).toString().padStart(2, "0");
  const m = Math.floor(seconds / 60) % 60;
  const h = Math.floor(seconds / 3600);
  return h > 0 ? `${h}:${m.toString().padStart(2, "0")}:${s}` : `${m}:${s}`;
}

export function mediaTitle(fileName) {
  if (!fileName) return "Seu filme, juntos";
  const dot = fileName.lastIndexOf(".");
  const stem = dot > 0 ? fileName.slice(0, dot) : fileName;
  return stem.trim() || fileName;
}

export function setEmblem(el, state, diameter) {
  if (!el) return;
  el.dataset.state = state;
  const r = diameter / 2;
  const offset = state === "together" ? 0.6
    : state === "waiting" || state === "lost" ? 1.25
    : state === "apart" ? 1.6
    : 0;
  const span = state === "solo" ? 1 : 1 + offset;
  const width = Math.ceil(diameter * span);
  const svg = el.querySelector("svg");
  if (!svg) return;
  svg.setAttribute("viewBox", `0 0 ${width} ${diameter}`);
  svg.setAttribute("width", String(width));
  svg.setAttribute("height", String(diameter));
  const [a, b] = svg.querySelectorAll("circle");
  if (a) {
    a.setAttribute("cx", String(r));
    a.setAttribute("cy", String(r));
    a.setAttribute("r", String(r));
  }
  if (b) {
    b.setAttribute("cx", String(r + diameter * offset));
    b.setAttribute("cy", String(r));
    b.setAttribute("r", String(state === "together" ? r : Math.max(1, r - 0.75)));
  }
}

export function peerPresence(view, stream) {
  const isHost = view.role === "host";
  const name = view.peer || (isHost ? "Convidado" : "Outra pessoa");
  if (isHost && !view.connected && !view.peer_left && !view.peer) {
    return { name: "Sala aberta", detail: "aguardando…", state: "waiting", warning: false };
  }
  if (view.peer_left && !view.connected) {
    return { name, detail: "saiu da sala", state: "solo", warning: false };
  }
  if (!view.connected) {
    return { name, detail: "reconectando", state: "lost", warning: true };
  }
  const peerBuffering = isHost ? view.guest_buffering : view.host_buffering;
  if (peerBuffering) {
    return { name, detail: "aguardando vídeo…", state: "waiting", warning: true };
  }
  if (view.buffering || view.waiting_buffer) {
    return { name, detail: "carregando vídeo…", state: "waiting", warning: true };
  }
  if (stream && (stream.phase === "preparing" || stream.phase === "negotiating" || stream.phase === "connecting")) {
    return { name, detail: "conectando P2P…", state: "waiting", warning: true };
  }
  if (view.preparing || view.scheduled) {
    return { name, detail: "preparando…", state: "waiting", warning: false };
  }
  if (!view.clock_ready) {
    return { name, detail: "acertando relógios…", state: "waiting", warning: false };
  }
  return { name, detail: "em sincronia", state: "together", warning: false };
}

export function centerLoadingState(view, stream) {
  const isHost = view.role === "host";
  const peer = view.peer || "a outra pessoa";
  if (stream && stream.phase === "failed" && stream.error) {
    return { title: "Conexão P2P interrompida", detail: stream.error, spinner: false };
  }
  if (stream && (stream.phase === "preparing" || stream.phase === "negotiating" || stream.phase === "connecting")) {
    return { title: "Conectando streaming P2P…", detail: stream.describe(), spinner: true };
  }
  if (view.buffering) {
    return { title: "Carregando vídeo…", detail: stream ? stream.describe() : "", spinner: true };
  }
  const peerBuffering = isHost ? view.guest_buffering : view.host_buffering;
  if (peerBuffering) {
    return { title: `Aguardando ${peer} carregar o vídeo…`, detail: "", spinner: true };
  }
  if (view.waiting_buffer) {
    return { title: "Carregando para retomar…", detail: "", spinner: true };
  }
  return null;
}

export function centerNoticeState(view, loading) {
  if (!view.loaded || loading || view.error || view.preparing || view.scheduled || view.blocked) {
    return null;
  }
  const isHost = view.role === "host";
  const peer = view.peer || (isHost ? "Convidado" : "Outra pessoa");
  if (view.peer_left) {
    return {
      dim: "lost",
      state: "solo",
      diameter: 28,
      title: `${peer} saiu da sala`,
      detail: isHost
        ? "A sala continua aberta. Aguardando outra pessoa entrar."
        : "A sala foi encerrada pelo anfitrião.",
    };
  }
  if (!view.connected) {
    return {
      dim: "lost",
      state: isHost && !view.peer ? "waiting" : "lost",
      diameter: 28,
      title: isHost && !view.peer ? "Aguardando convidado" : "Conexão perdida",
      detail: isHost
        ? (view.peer ? `Aguardando ${view.peer} reconectar…` : "Envie o código para alguém entrar na sala.")
        : `Reconectando com ${view.peer || "a outra pessoa"}…`,
    };
  }
  if (view.paused) {
    const selfSide = isHost ? "Host" : "Guest";
    const otherSide = isHost ? "Guest" : "Host";
    const title = view.paused_by === selfSide
      ? "Você pausou"
      : view.paused_by === otherSide
      ? `${peer} pausou`
      : "Pausado";
    return {
      dim: "paused",
      state: "together",
      diameter: 22,
      title,
      detail: `em ${formatTime(view.position)}`,
    };
  }
  return null;
}

export function videoBannerState(view, appNotice, transientNotice) {
  if (view.error) {
    return { text: view.error, kind: "alert" };
  }
  if (view.preparing || view.scheduled) {
    return { text: "Preparando a reprodução…", kind: "spinner" };
  }
  if (view.blocked && !view.buffering) {
    return { text: "A reprodução foi interrompida. Aguardando o player…", kind: "alert" };
  }
  const text = appNotice || view.notice || transientNotice;
  if (text) {
    const warning = Boolean(appNotice);
    return { text, kind: warning ? "alert" : "info" };
  }
  return null;
}

export { SVG_NS };
