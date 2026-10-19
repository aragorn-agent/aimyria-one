// Local release server for exercising quickstart.ps1 without publishing.
//
// Serves a directory over HTTP and prints the matching BaseUrl, so the install
// and uninstall scripts can be run end to end against real files. Use this
// before publishing: the install path is the first thing a user touches, and
// "it worked on my machine" has to mean something.
'use strict';
import { createServer } from "node:http";
import { createReadStream, existsSync, statSync } from "node:fs";
import { resolve } from "node:path";

const root = process.argv[2];
const port = Number(process.argv[3] ?? 8787);

if (!root || !existsSync(root)) {
  console.error("usage: node tools/serve_release.mjs <dir> [port]");
  process.exit(2);
}

createServer((req, res) => {
  // The whole path is significant: quickstart.ps1 requests
  // `/releases/latest/download/<name>`, and the files live under exactly that
  // prefix inside the served directory. Taking only the last segment would 404
  // on everything.
  const rel = decodeURIComponent((req.url ?? "/").split("?")[0]).replace(/^\/+/, "");
  const file = resolve(root, rel);

  // Traversal guard, after resolution: `resolve` collapses `..`, so anything
  // that still escapes `root` is a request we refuse rather than serve.
  if (!file.startsWith(resolve(root)) || !existsSync(file) || !statSync(file).isFile()) {
    res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    res.end("not found\n");
    console.log(`  404 ${req.url}`);
    return;
  }

  // Content type matters: PowerShell's Invoke-WebRequest hands back `.Content`
  // as a byte[] for anything that is not text, and the install script calls
  // `.Trim()` on a checksum. Serving a real release, not a file dump.
  const type = file.endsWith(".sha256") || file.endsWith(".txt")
    ? "text/plain; charset=utf-8"
    : "application/octet-stream";

  res.writeHead(200, { "content-type": type });
  createReadStream(file).pipe(res);
  console.log(`  200 ${req.url}`);
}).listen(port, () => {
  console.log(`serving ${root}`);
  console.log(`BaseUrl = http://127.0.0.1:${port}`);
});