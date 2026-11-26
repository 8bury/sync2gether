const WAIT_MS = 60000;
const STREAM = /\/stream\/(\d+)\/[^/]+$/;

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin) return;
  const match = url.pathname.match(STREAM);
  if (!match || event.request.method !== "GET") return;
  event.respondWith(serve(event, Number(match[1])));
});

function parseRange(header) {
  const match = /^bytes=(\d+)-(\d*)$/.exec((header || "bytes=0-").trim());
  if (!match) return null;
  const start = Number(match[1]);
  const end = match[2] === "" ? Number.MAX_SAFE_INTEGER : Number(match[2]);
  return end >= start ? { start, end } : null;
}

function ask(client, id, range) {
  return new Promise((resolve) => {
    const channel = new MessageChannel();
    const timer = setTimeout(() => resolve(null), WAIT_MS);
    channel.port1.onmessage = (event) => {
      clearTimeout(timer);
      resolve(event.data && !event.data.missing ? event.data : null);
    };
    client.postMessage({ type: "range", id, start: range.start, end: range.end }, [channel.port2]);
  });
}

async function serve(event, id) {
  const range = parseRange(event.request.headers.get("range"));
  if (!range) return new Response(null, { status: 416 });
  let clients = [];
  const own = event.clientId ? await self.clients.get(event.clientId) : null;
  if (own) {
    clients = [own];
  } else {
    clients = await self.clients.matchAll({ type: "window" });
  }
  for (const client of clients) {
    const answer = await ask(client, id, range);
    if (!answer) continue;
    const length = answer.bytes.byteLength;
    if (range.start >= answer.total) {
      return new Response(null, {
        status: 416,
        headers: { "Content-Range": `bytes */${answer.total}` },
      });
    }
    return new Response(answer.bytes, {
      status: 206,
      headers: {
        "Content-Type": answer.mime,
        "Content-Length": String(length),
        "Content-Range": `bytes ${range.start}-${range.start + length - 1}/${answer.total}`,
        "Accept-Ranges": "bytes",
        "Cache-Control": "no-store",
      },
    });
  }
  return new Response(null, { status: 504 });
}
