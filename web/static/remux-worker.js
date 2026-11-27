import init, { WasmMkvRemuxer } from "./pkg/sync2gether_web.js";

const ready = init();
const reader = typeof FileReaderSync !== "undefined" ? new FileReaderSync() : null;
let remuxer = null;

function closeCurrent() {
  if (remuxer) {
    try { remuxer.free(); } catch {}
    remuxer = null;
  }
}

self.onmessage = async (event) => {
  const msg = event.data;
  try {
    await ready;
    if (msg.type === "close") {
      closeCurrent();
      return;
    }
    if (msg.type === "open") {
      closeCurrent();
      if (!reader) {
        self.postMessage({
          id: msg.id,
          ok: false,
          error: { kind: "invalid_file", detail: "FileReaderSync indisponível neste navegador." },
        });
        return;
      }
      const file = msg.file;
      const readAt = (offset, len) => {
        const end = Math.min(file.size, offset + len);
        return new Uint8Array(reader.readAsArrayBuffer(file.slice(offset, end)));
      };
      remuxer = WasmMkvRemuxer.open(file.size, readAt);
      const mse = JSON.parse(remuxer.mse_info());
      const skippedAudio = JSON.parse(remuxer.skipped_audio_codecs());
      self.postMessage({ id: msg.id, ok: true, mse, skippedAudio });
      return;
    }
    if (!remuxer) {
      self.postMessage({ id: msg.id, ok: false, error: "Remux fechado." });
      return;
    }
    if (msg.type === "index") {
      const bytes = remuxer.encoded_index();
      self.postMessage({ id: msg.id, ok: true, bytes }, [bytes.buffer]);
    } else if (msg.type === "init") {
      const bytes = remuxer.init_segment(msg.track);
      self.postMessage({ id: msg.id, ok: true, bytes }, [bytes.buffer]);
    } else if (msg.type === "segment") {
      const bytes = remuxer.media_segment(msg.track, msg.segment);
      self.postMessage({ id: msg.id, ok: true, bytes }, [bytes.buffer]);
    }
  } catch (err) {
    const raw = String(err && err.message || err);
    let parsed = raw;
    try { parsed = JSON.parse(raw); } catch {}
    self.postMessage({ id: msg.id, ok: false, error: parsed });
  }
};
