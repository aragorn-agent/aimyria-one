// Local release server for exercising the silent self-updater.
//
// WHY THIS EXISTS, SEPARATELY FROM tools/serve_release.mjs
//
// That script serves an install without publishing, and its content-type map
// sends everything that is not .sha256/.txt out as application/octet-stream --
// including .json. It is the right harness for quickstart.ps1 and the wrong one
// for the updater, for one measured reason: the update channel's failure mode is
// a cached manifest. `version.json` is fetched every six hours by every installed
// copy, through whatever CDN sits in front of aimyria.com, and if that response
// is cacheable the fleet never sees a new release. Reproducing the real headers
// locally is the only way to test the thing that actually breaks, so this server
// mirrors them:
//
//   version.json  -> application/json; charset=utf-8,  Cache-Control: no-store
//   *.exe         -> application/octet-stream, Content-Disposition: attachment
//   *.sha256/.txt -> text/plain; charset=utf-8
//
// It also logs every request, so two stale reads of the manifest six hours apart
// are distinguishable from one.
//
// WHAT IT DELIBERATELY DOES NOT DO
//
// It is not hardened: it binds loopback, it serves one directory, and it is
// meant to be started and stopped inside an acceptance run. It has no place on a
// public interface.
'use strict';
import { createServer } from "node:http";
import { createReadStream, existsSync, statSync } from "node:fs";
import { resolve, extname } from "node:path";

const root = process.argv[2];
const port = Number(process.argv[3] ?? 8799);

if (!root || !existsSync(root)) {
  console.error("usage: node scripts/serve_updater_release.mjs <dir> <port>");
  process.exit(2);
}

const ROOT = resolve(root);

function contentType(name) {
  const ext = extname(name).toLowerCase();
  if (ext === ".json") return "application/json; charset=utf-8";
  if (ext === ".sha256" || ext === ".txt") return "text/plain; charset=utf-8";
  return "application/octet-stream";
}

let served = 0;

createServer((req, res) => {
  const url = new URL(req.url ?? "/", `http://127.0.0.1:${port}`);
  const rel = decodeURIComponent(url.pathname).replace(/^\/+/, "");
  const file = resolve(ROOT, rel);

  // Traversal guard after resolution: `resolve` collapses `..`, so anything still
  // outside ROOT is a request to refuse rather than serve.
  if (!file.startsWith(ROOT) || !existsSync(file) || !statSync(file).isFile()) {
    res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
    res.end("not found\n");
    console.log(`  404 ${url.pathname}`);
    return;
  }

  const headers = {
    "content-type": contentType(file),
    "content-length": String(statSync(file).size),
  };

  if (url.pathname.endsWith(".json")) {
    // The header that decides whether the fleet ever sees a new release.
    headers["cache-control"] = "no-store";
  }
  if (url.pathname.endsWith(".exe")) {
    // A release must be *downloaded*, not rendered into a browser tab.
    headers["content-disposition"] =
      `attachment; filename="${url.pathname.split("/").pop()}"`;
  }

  served++;
  res.writeHead(200, headers);
  if (req.method === "HEAD") {
    res.end();
  } else {
    createReadStream(file).pipe(res);
  }
  console.log(`  200 ${url.pathname} (${headers["content-type"]})`);
}).listen(port, "127.0.0.1", () => {
  console.log(`serving ${ROOT}`);
  console.log(`base = http://127.0.0.1:${port}`);
  console.log(`manifest = http://127.0.0.1:${port}/releases/latest/download/version.json`);
});

// A run that leaves the server up is a run that lies to the next one; report the
// count so an acceptance log shows the manifest was actually fetched.
process.on("SIGINT", () => {
  console.log(`stopping after ${served} request(s)`);
  process.exit(0);
});
