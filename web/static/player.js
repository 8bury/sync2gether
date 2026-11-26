const HAVE_FUTURE_DATA = 3;
const START_SPIN_MS = 25;

export function now() {
  return performance.now();
}

export class Player {
  constructor(video, hooks) {
    this.video = video;
    this.hooks = hooks;
    this.generation = 0;
    this.loaded = false;
    this.waiting = false;
    this.incompatible = null;
    this.preparing = null;
    this.prepared = null;
    this.timer = null;
    this.silentWarned = false;
    for (const name of ["waiting", "stalled"]) {
      video.addEventListener(name, () => { this.waiting = !video.paused || this.preparing !== null; });
    }
    for (const name of ["playing", "canplay", "canplaythrough", "seeked"]) {
      video.addEventListener(name, () => {
        if (video.readyState >= HAVE_FUTURE_DATA) this.waiting = false;
        this.checkPrepared();
      });
    }
    video.addEventListener("loadedmetadata", () => this.inspect());
    video.addEventListener("timeupdate", () => this.checkAudio());
    video.addEventListener("error", () => {
      if (!this.video.getAttribute("src")) return;
      this.fail("O navegador não conseguiu abrir este arquivo. Tente um MP4 (H.264 e AAC) ou use o app.");
    });
  }

  load(url, generation) {
    this.cancelSchedule();
    this.generation = generation;
    this.loaded = false;
    this.waiting = false;
    this.incompatible = null;
    this.silentWarned = false;
    this.preparing = null;
    this.prepared = null;
    this.video.pause();
    this.video.src = url;
    this.video.load();
  }

  unload(generation) {
    this.cancelSchedule();
    this.generation = generation;
    this.loaded = false;
    this.preparing = null;
    this.prepared = null;
    this.video.pause();
    this.video.removeAttribute("src");
    this.video.load();
  }

  inspect() {
    const video = this.video;
    if (video.videoWidth === 0 && video.videoHeight === 0) {
      this.fail("O navegador não encontrou uma faixa de vídeo que consiga tocar neste arquivo. Use o app ou um MP4 (H.264 e AAC).");
      return;
    }
    this.loaded = true;
    this.hooks.loaded?.();
  }

  checkAudio() {
    const video = this.video;
    if (!this.loaded || video.paused || video.currentTime < 2) return;
    const silent = (typeof video.mozHasAudio === "boolean" && !video.mozHasAudio) ||
      (typeof video.webkitAudioDecodedByteCount === "number" && video.webkitAudioDecodedByteCount === 0 &&
        (video.audioTracks === undefined || video.audioTracks.length > 0));
    if (silent && !this.silentWarned) {
      this.silentWarned = true;
      this.hooks.notice?.("O navegador não está tocando o áudio deste arquivo (AC-3 e DTS não funcionam na maioria dos navegadores). Use o app para ter som.");
    }
  }

  fail(message) {
    this.incompatible = message;
    this.loaded = false;
    this.hooks.error?.(message);
  }

  bufferedUntil() {
    const { buffered, currentTime } = this.video;
    for (let i = 0; i < buffered.length; i++) {
      if (buffered.start(i) <= currentTime + 0.05 && buffered.end(i) >= currentTime) {
        return buffered.end(i);
      }
    }
    return currentTime;
  }

  status() {
    const video = this.video;
    const duration = Number.isFinite(video.duration) ? video.duration : 0;
    return {
      generation: this.generation,
      measured_ms: now(),
      position: video.currentTime || 0,
      duration,
      speed: video.playbackRate || 1,
      paused: video.paused,
      seeking: video.seeking,
      loaded: this.loaded && this.incompatible === null,
      buffering: this.waiting && this.loaded,
      blocked: this.incompatible !== null,
      ended: video.ended,
      buffered_until: this.bufferedUntil(),
    };
  }

  run(command) {
    const video = this.video;
    switch (command.kind) {
      case "prepare":
        this.cancelSchedule();
        this.prepared = null;
        this.preparing = command.token;
        video.pause();
        video.playbackRate = 1;
        if (Math.abs(video.currentTime - command.position) > 0.001) {
          video.currentTime = command.position;
        }
        queueMicrotask(() => this.checkPrepared());
        break;
      case "schedule":
        this.schedule(command.token, command.at);
        break;
      case "pause":
        if (command.paused) {
          this.cancelSchedule();
          video.pause();
        } else {
          video.play().catch(() => {});
        }
        break;
      case "seek":
        this.cancelSchedule();
        video.currentTime = Math.max(0, command.position);
        break;
      case "speed":
        video.playbackRate = command.speed;
        break;
    }
  }

  checkPrepared() {
    const video = this.video;
    if (this.preparing === null || video.seeking || video.readyState < HAVE_FUTURE_DATA) return;
    const token = this.preparing;
    this.preparing = null;
    this.prepared = token;
    this.hooks.prepared?.(token);
  }

  cancelSchedule() {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  schedule(token, at) {
    this.cancelSchedule();
    if (this.prepared !== token || at <= now()) {
      this.hooks.missed?.(token);
      return;
    }
    const fire = () => {
      this.timer = null;
      while (now() < at) {}
      this.start(token);
    };
    const wait = at - now() - START_SPIN_MS;
    if (wait > 0) {
      this.timer = setTimeout(() => {
        const left = at - now() - START_SPIN_MS;
        if (left > 4) {
          this.timer = setTimeout(fire, left);
        } else {
          fire();
        }
      }, wait);
    } else {
      fire();
    }
  }

  start(token) {
    const video = this.video;
    const at = now();
    video.play().then(
      () => this.hooks.started?.(token, at),
      (error) => {
        if (error && error.name === "NotAllowedError" && !video.muted) {
          video.muted = true;
          video.play().then(
            () => {
              this.hooks.muted?.();
              this.hooks.started?.(token, at);
            },
            () => this.blocked(token),
          );
        } else {
          this.blocked(token);
        }
      },
    );
  }

  blocked(token) {
    this.hooks.unlock?.();
    this.hooks.missed?.(token);
  }

  unlock() {
    const video = this.video;
    const wasPaused = video.paused;
    return video.play().then(() => { if (wasPaused) video.pause(); }, () => {});
  }
}
