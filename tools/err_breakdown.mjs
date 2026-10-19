// Categorise cargo errors so we can tell "cheap to fix" from "needs the deleted proxy".
//
// PowerShell mangles regexes containing backticks (its escape character), which is
// how the first attempt at this produced fifty parse errors instead of a
// breakdown. Node has no such rule.

import { execFileSync } from "node:child_process";

const env = {
  ...process.env,
  RUSTUP_HOME: "D:\\Rust\\rustup",
  CARGO_HOME: "D:\\Rust\\cargo",
  RUSTUP_TOOLCHAIN: "1.95.0-x86_64-pc-windows-gnu",
  CC: "gcc",
  AR: "ar",
  CARGO_TERM_COLOR: "never",
  CARGO_TARGET_DIR: "D:\\Rust\\build\\aimyria-one",
  PATH: `${process.env.CARGO_HOME || "D:\\Rust\\cargo"}\\bin;D:\\Tools\\mingw64\\mingw64\\bin;${process.env.PATH}`,
};

let raw = "";
try {
  raw = execFileSync("cargo", ["check", "--lib", "--message-format", "short"], {
    cwd: "D:\\src\\cc-switch-main\\src-tauri",
    encoding: "utf8",
    env,
    timeout: 1800000,
    maxBuffer: 64 * 1024 * 1024,
  });
} catch (e) {
  raw = `${e.stdout || ""}${e.stderr || ""}`;
}

const lines = raw.split(/\r?\n/).filter((l) => l.includes(": error"));

// A proxy error is one that can only be resolved by importing the local routing
// proxy this build deliberately does not have.
const PROXY = /\bproxy\b|ProxyService|copilot|codex_oauth|OAuth/;

const proxy = lines.filter((l) => PROXY.test(l));
const rest = lines.filter((l) => !PROXY.test(l));

console.log(`total errors      : ${lines.length}`);
console.log(`need the proxy    : ${proxy.length}`);
console.log(`fixable without it: ${rest.length}`);
console.log("");

// What is actually missing, for the fixable group.
const missing = new Map();
for (const l of rest) {
  let key = "(other)";
  let m = l.match(/cannot find `([^`]+)`/);
  if (m) key = `cannot find ${m[1]}`;
  m = l.match(/no (?:field|function|method|value) `([^`]+)`/);
  if (m) key = `missing ${m[1]}`;
  m = l.match(/unresolved import `([^`]+)`/);
  if (m) key = `import ${m[1]}`;
  if (/unclosed delimiter/.test(l)) key = "unclosed delimiter";
  if (!missing.has(key)) missing.set(key, { n: 0, files: new Set() });
  const e = missing.get(key);
  e.n += 1;
  const f = l.split(":")[0];
  e.files.add(f);
}

console.log("fixable group, by cause:");
for (const [k, v] of [...missing.entries()].sort((a, b) => b[1].n - a[1].n)) {
  console.log(`  ${String(v.n).padStart(3)}  ${k}`);
  if (v.n > 1) {
    console.log(`       in ${[...v.files].slice(0, 4).join(", ")}${v.files.size > 4 ? ` (+${v.files.size - 4})` : ""}`);
  }
}
console.log("");
console.log("proxy-gated, by file:");
const pf = new Map();
for (const l of proxy) {
  const f = l.split(":")[0];
  pf.set(f, (pf.get(f) || 0) + 1);
}
for (const [f, n] of [...pf.entries()].sort((a, b) => b[1] - a[1])) {
  console.log(`  ${String(n).padStart(3)}  ${f}`);
}
