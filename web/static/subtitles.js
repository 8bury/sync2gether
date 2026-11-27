const MAX_SUBTITLE_BYTES = 1024 * 1024;
const MIN_DELAY_MS = -60000;
const MAX_DELAY_MS = 60000;

export function formatDelay(delayMs) {
  const seconds = delayMs / 1000;
  const sign = delayMs > 0 ? "+" : "";
  return `${sign}${seconds.toFixed(1)} s`;
}

export class Subtitles {
  constructor(video, hooks) {
    this.video = video;
    this.hooks = hooks || {};
    this.host = null;
    this.local = null;
    this.active = "off";
    this.delayMs = 0;
    this.error = null;
    this.trackEl = null;
    this.trackUrl = null;
  }

  hasAny() {
    return Boolean(this.host || this.local);
  }

  isShowing() {
    return this.active !== "off" && this.currentSource() !== null;
  }

  currentSource() {
    if (this.active === "host") return this.host;
    if (this.active === "local") return this.local;
    return null;
  }

  async loadLocalFile(file) {
    if (!file) return false;
    this.error = null;
    if (file.size > MAX_SUBTITLE_BYTES) {
      this.error = "A legenda é grande demais (limite de 1 MiB).";
      this.hooks.onChange?.();
      return false;
    }
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const parse = this.hooks.parse;
      if (!parse) {
        throw new Error("Conversor de legenda indisponível.");
      }
      const vtt = parse(bytes);
      this.local = { name: file.name, vtt };
      this.active = "local";
      this.delayMs = 0;
      this.applyTrack();
      this.hooks.onChange?.();
      return true;
    } catch (error) {
      this.error = String((error && error.message) || error || "Não foi possível ler esta legenda.");
      this.hooks.onChange?.();
      return false;
    }
  }

  setHost(name, vtt) {
    this.error = null;
    this.host = { name, vtt };
    if (this.active !== "local") {
      this.active = "host";
      this.delayMs = 0;
    }
    this.applyTrack();
    this.hooks.onChange?.();
  }

  clearHost() {
    this.host = null;
    if (this.active === "host") {
      this.active = this.local ? "local" : "off";
      this.delayMs = 0;
    }
    this.applyTrack();
    this.hooks.onChange?.();
  }

  select(source) {
    this.error = null;
    if (source === "host" && this.host) {
      this.active = "host";
    } else if (source === "local" && this.local) {
      this.active = "local";
    } else {
      this.active = "off";
    }
    this.applyTrack();
    this.hooks.onChange?.();
  }

  adjustDelay(deltaMs) {
    const next = Math.max(MIN_DELAY_MS, Math.min(MAX_DELAY_MS, this.delayMs + deltaMs));
    if (next === this.delayMs) return;
    this.delayMs = next;
    this.applyTrack();
    this.hooks.onChange?.();
  }

  resetDelay() {
    if (this.delayMs === 0) return;
    this.delayMs = 0;
    this.applyTrack();
    this.hooks.onChange?.();
  }

  clearAll() {
    this.host = null;
    this.local = null;
    this.active = "off";
    this.delayMs = 0;
    this.error = null;
    this.removeTrack();
    this.hooks.onChange?.();
  }

  removeTrack() {
    if (this.trackEl) {
      if (this.trackEl.track) {
        this.trackEl.track.mode = "disabled";
      }
      this.trackEl.remove();
      this.trackEl = null;
    }
    if (this.trackUrl) {
      URL.revokeObjectURL(this.trackUrl);
      this.trackUrl = null;
    }
  }

  applyTrack() {
    this.removeTrack();
    const source = this.currentSource();
    if (!source) return;
    const vtt = this.delayMs !== 0 && this.hooks.shift
      ? this.hooks.shift(source.vtt, this.delayMs)
      : source.vtt;
    const blob = new Blob([vtt], { type: "text/vtt;charset=utf-8" });
    this.trackUrl = URL.createObjectURL(blob);
    const track = document.createElement("track");
    track.kind = "subtitles";
    track.label = source.name;
    track.src = this.trackUrl;
    track.default = true;
    this.video.appendChild(track);
    this.trackEl = track;
    if (track.track) {
      track.track.mode = "showing";
    }
    track.addEventListener("load", () => {
      if (this.trackEl === track && track.track) {
        track.track.mode = "showing";
      }
    });
  }
}
