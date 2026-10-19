// Flatten an MSVC `.lib` (a plain `!<arch>` archive) into GNU-compatible object
// files with no directory structure.
//
// Why this exists: the WebView2 SDK's `WebView2LoaderStatic.lib` stores its members
// under paths like `obj/edge_embedded_browser/client/win/WebView2LoaderLib/…`,
// and `ar x` fails on those under MinGW because the parent directories do not
// exist (`obj/…: No such file or directory`). MSVC's own linker creates them; GNU
// `ar` will not.
//
// Rewriting the members as `m0000.obj` … is safe: the archive is a link input, and
// nothing in a COFF object references its siblings by path. Section names inside
// each object are what the linker matches on, and those are untouched.
//
// Format handled: the System V / GNU variant, which is what `!<arch>` with a
// 4-member name table ("/" or "//") is. MSVC uses short names ending in "/" and a
// GNU-style long-name table; both are covered by reading the size field and
// honouring the GNU "//" long-name table when present.

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const [, , src, dst] = process.argv;
if (!src || !dst) {
  console.error("usage: extract_coff_lib.mjs <in.lib> <outdir>");
  process.exit(1);
}

mkdirSync(dst, { recursive: true });
const buf = readFileSync(src);

const MAGIC = "!<arch>\n";
if (buf.subarray(0, 8).toString("latin1") !== MAGIC) {
  throw new Error(`not an ar archive: ${src}`);
}

// Long names GNU ar stores in a `//` member, NUL-terminated and concatenated.
let longNames = Buffer.alloc(0);
const members = [];
let off = 8;
while (off + 60 <= buf.length) {
  const header = buf.subarray(off, off + 60);
  const nameField = header.subarray(0, 16).toString("latin1");
  const sizeField = header.subarray(48, 58).toString("latin1").trim();
  const size = parseInt(sizeField, 10);
  if (!Number.isFinite(size)) break;
  const bodyStart = off + 60;
  const body = buf.subarray(bodyStart, bodyStart + size);
  off = bodyStart + size + (size % 2); // members are 2-byte aligned

  let raw = nameField.trim();
  if (raw === "/" || raw === "" || raw === "/SYM64/") {
    members.push({ name: `special-${members.length}`, body });
    continue;
  }
  if (raw === "//") {
    longNames = body;
    members.push({ name: "//", body });
    continue;
  }
  // `/123` indexes the long-name table; trailing `/` is the MSVC terminator.
  const slash = raw.indexOf("/");
  const idx = slash === -1 ? null : parseInt(raw.slice(0, slash), 10);
  if (idx !== null && longNames.length) {
    let end = longNames.indexOf(0, idx);
    if (end === -1) end = longNames.length;
    raw = longNames.subarray(idx, end).toString("latin1").replace(/\/$/, "");
  }
  members.push({ name: raw.replace(/\/$/, ""), body });
}

let n = 0;
const written = [];
for (const m of members) {
  if (m.name === "//" || m.name.startsWith("special-")) continue;
  const leaf = m.name.split(/[\\/]/).pop();
  if (!leaf || !/\.(obj|o)$/i.test(leaf)) continue;
  const flat = `m${String(n).padStart(4, "0")}.obj`;
  writeFileSync(join(dst, flat), m.body);
  written.push({ flat, from: m.name });
  n += 1;
}

console.log(`  ${n} objects extracted from ${members.length} members`);
if (process.argv[3] === "--list") {
  for (const w of written.slice(0, 8)) console.log(`    ${w.flat}  <- ${w.from}`);
}
