import {
  bindEvents,
  setAudioTracks,
  setSubtitleMenuOpen,
  show,
  showEnded,
  showEntry,
  showRoom,
  start,
  subtitles,
} from "./app.js";

function previewFixture(scenario) {
  const base = {
    code: "4821-gato-lua",
    connected: true,
    reconnecting: false,
    ended: false,
    peer: "Ana",
    error: null,
    notice: null,
    clock_ready: true,
    rtt_ms: 28,
    host_media: { hash: "a".repeat(64), bytes: 1_450_000_000 },
    host_file_name: "Filme.de.demonstracao.1080p.mp4",
    host_duration: 5400,
    host_ready: true,
    host_loading: false,
    host_buffering: false,
    host_blocked: false,
    host_web_stream: true,
    host_web_remux: true,
    host_position: 754,
    guest_media: { hash: "a".repeat(64), bytes: 1_450_000_000 },
    guest_file_name: "Filme de demonstracao.mp4",
    matched: true,
    mismatch: false,
    paused: false,
    paused_by: null,
    playing: true,
    position: 754,
    buffered_until: 1480,
    loaded: true,
    watching: true,
    ready: true,
    blocked: false,
    buffering: false,
    waiting_buffer: false,
    preparing: false,
    scheduled: false,
    peer_left: false,
    scheduled_start_ms: null,
    drift_ms: 14,
    generation: 1,
  };
  switch (scenario) {
    case "prepare":
      return {
        ...base,
        guest_media: null,
        guest_file_name: null,
        matched: false,
        playing: false,
        paused: true,
        loaded: false,
        watching: false,
        ready: false,
        drift_ms: null,
      };
    case "paused":
      return {
        ...base,
        paused: true,
        playing: false,
        paused_by: "Host",
        notice: "Conexão restabelecida. Aperte reproduzir para continuar.",
      };
    case "reconnecting":
      return {
        ...base,
        connected: false,
        reconnecting: true,
        paused: true,
        playing: false,
        drift_ms: null,
      };
    case "buffering":
      return {
        ...base,
        host_buffering: true,
        waiting_buffer: true,
        paused: true,
        playing: false,
      };
    case "mkv-prepare":
      return {
        ...base,
        host_file_name: "Filme.de.demonstracao.1080p.AC3.mkv",
        host_web_remux: true,
        guest_media: null,
        guest_file_name: null,
        matched: false,
        playing: false,
        paused: true,
        loaded: false,
        watching: false,
        ready: false,
        drift_ms: null,
      };
    case "mkv-prepare-fallback":
      return {
        ...base,
        host_file_name: "Filme.de.demonstracao.1080p.AC3.mkv",
        host_web_remux: false,
        guest_media: null,
        guest_file_name: null,
        matched: false,
        playing: false,
        paused: true,
        loaded: false,
        watching: false,
        ready: false,
        drift_ms: null,
      };
    case "mkv-streaming":
    case "audio-menu":
      return {
        ...base,
        host_file_name: "Filme.de.demonstracao.1080p.AC3.mkv",
        guest_file_name: "Filme.de.demonstracao.1080p.AC3.mkv",
      };
    case "player":
    default:
      return base;
  }
}

function runPreview(scenario) {
  const theme = new URLSearchParams(location.search).get("theme") || "dark";
  if (theme === "light" || theme === "dark") {
    document.documentElement.dataset.theme = theme;
  }
  if (scenario === "entry") {
    showEntry("");
    return;
  }
  if (scenario === "connecting") {
    show("connecting");
    return;
  }
  if (scenario === "ended") {
    showEnded("Sala encerrada", "Ana encerrou a sala.");
    return;
  }
  document.getElementById("stage").classList.add("preview-backdrop");
  showRoom(previewFixture(scenario));
  if (scenario === "mkv-streaming" || scenario === "audio-menu") {
    const sampleVtt = "WEBVTT\n\n00:00:00.000 --> 02:00:00.000\nO mesmo filme, no mesmo segundo.\n";
    subtitles.setEmbeddedTracks([
      { index: 0, language: "por", title: "Português (SRT)", codec: "subrip", default: true },
      { index: 1, language: "eng", title: "English (ASS)", codec: "ass", default: false },
    ]);
    subtitles.setEmbeddedVtt(0, "Português (SRT)", sampleVtt);
    subtitles.setHost("Filme.de.demonstracao.pt-BR.srt", sampleVtt);
    subtitles.selectEmbedded(0);
    setAudioTracks(
      [
        { index: 0, language: "por", title: "Português (Dublado)", codec: "ac3", channels: 6, default: true },
        { index: 1, language: "eng", title: "Original", codec: "dts", channels: 6, default: false },
      ],
      0,
    );
    const hud = document.getElementById("stream-hud");
    hud.hidden = false;
    hud.textContent = "Streaming adaptado (6.4 Mbit/s)";
    setSubtitleMenuOpen(scenario === "audio-menu");
  } else if (scenario === "subtitles" || scenario === "subtitles-menu") {
    const sampleVtt = "WEBVTT\n\n00:00:00.000 --> 02:00:00.000\nO mesmo filme, no mesmo segundo.\n";
    subtitles.setHost("Filme.de.demonstracao.pt-BR.srt", sampleVtt);
    subtitles.delayMs = 500;
    const cue = document.createElement("div");
    cue.className = "preview-cue";
    cue.textContent = "O mesmo filme, no mesmo segundo.";
    document.getElementById("stage").appendChild(cue);
    setSubtitleMenuOpen(scenario === "subtitles-menu");
  }
}

const previewScenario = new URLSearchParams(location.search).get("preview");
if (previewScenario) {
  bindEvents();
  runPreview(previewScenario);
} else {
  start();
}
