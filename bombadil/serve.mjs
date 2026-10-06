// A static file server for the report test, with the byte ranges browsers need to seek in
// audio. Prints its URL once listening.
//
//   node serve.mjs DIR [PORT]

import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import { extname, join, normalize } from "node:path";

const [root, port = "0"] = process.argv.slice(2);
const TYPES = { ".html": "text/html; charset=utf-8", ".wav": "audio/wav", ".json": "application/json" };

createServer((req, res) => {
  const path = join(root, normalize(decodeURIComponent(new URL(req.url, "http://x").pathname)));
  let size;
  try {
    const st = statSync(path);
    if (!st.isFile()) throw new Error("not a file");
    size = st.size;
  } catch {
    res.writeHead(404).end();
    return;
  }
  const headers = { "Content-Type": TYPES[extname(path)] ?? "application/octet-stream", "Accept-Ranges": "bytes" };
  const range = /^bytes=(\d*)-(\d*)$/.exec(req.headers.range ?? "");
  if (range) {
    const start = range[1] ? Number(range[1]) : size - Number(range[2]);
    const end = range[1] && range[2] ? Math.min(Number(range[2]), size - 1) : size - 1;
    if (start > end || start >= size) {
      res.writeHead(416, { "Content-Range": `bytes */${size}` }).end();
      return;
    }
    res.writeHead(206, { ...headers, "Content-Range": `bytes ${start}-${end}/${size}`, "Content-Length": end - start + 1 });
    createReadStream(path, { start, end }).pipe(res);
  } else {
    res.writeHead(200, { ...headers, "Content-Length": size });
    createReadStream(path).pipe(res);
  }
}).listen(Number(port), "127.0.0.1", function () {
  console.log(`http://127.0.0.1:${this.address().port}`);
});
