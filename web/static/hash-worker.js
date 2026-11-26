import init, { FileHasher } from "./pkg/sync2gether_web.js";

const SLICE = 8 * 1024 * 1024;
const ready = init();

self.onmessage = async (event) => {
  const { id, file } = event.data;
  try {
    await ready;
    const hasher = new FileHasher();
    let done = 0;
    let last = 0;
    while (done < file.size) {
      const end = Math.min(file.size, done + SLICE);
      const bytes = new Uint8Array(await file.slice(done, end).arrayBuffer());
      hasher.update(bytes);
      done = end;
      const time = performance.now();
      if (time - last > 150 || done === file.size) {
        last = time;
        self.postMessage({ id, progress: done, total: file.size });
      }
    }
    const media = JSON.parse(hasher.finish());
    hasher.free();
    self.postMessage({ id, media });
  } catch (error) {
    self.postMessage({ id, error: String(error && error.message || error) });
  }
};
