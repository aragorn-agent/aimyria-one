// Option A for the OpenCode deep link: carry `npm` and `model` in the payload.
//
// # Why this was needed
//
// A one-click link that omits `model` fails OpenCode with
// "新的 OpenCode 供应商标识需要填写 npm 包和至少一个模型". The npm package was
// never the problem — `deeplink/provider.rs` has hardcoded
// `@ai-sdk/openai-compatible` since the start. The validator wants a *non-empty*
// `models` map, and `build_opencode_settings` only inserts one when the request
// carries a model. So the link had nothing to fail with: `models` came out `{}`.
//
// # The two defaults
//
//   npm   — `@ai-sdk/openai-compatible`, already the value in our own code, not
//           chosen here.
//   model — `gpt-5`. This one IS a choice, made here so one-click works without a
//           website change. It matches the OpenAI-compatible surface the npm
//           package implies. `AIMYRIA_OPENCODE_DEFAULT_MODEL` is the single place
//           to change it.
//
// Both are overridable: `&npm=...&model=...` on the link wins.
//
// # Method
//
// Splice by line index rather than matching text. lib.rs, and now these two files,
// are CRLF — four edits in this project have failed silently on `\n` anchors.

import { readFileSync, writeFileSync } from "node:fs";

const MOD = "D:/src/cc-switch-main/src-tauri/src/deeplink/mod.rs";
const PARSER = "D:/src/cc-switch-main/src-tauri/src/deeplink/parser.rs";
const PROVIDER = "D:/src/cc-switch-main/src-tauri/src/deeplink/provider.rs";

const done = [];

// ── 1. the request field ─────────────────────────────────────────────────────
{
  const lines = readFileSync(MOD, "utf8").split("\n");
  if (lines.some((l) => l.includes("pub npm:"))) {
    done.push("mod.rs: npm field already present");
  } else {
    const at = lines.findIndex((l) => l.trim() === "pub model: Option<String>,");
    if (at === -1) throw new Error("model field not found in mod.rs");
    // The serde attribute sits two lines above `pub model`.
    lines.splice(
      at + 1,
      0,
      '    /// Optional npm package for OpenCode (overrides the built-in default).',
      '    #[serde(skip_serializing_if = "Option::is_none")]',
      "    pub npm: Option<String>,",
    );
    writeFileSync(MOD, lines.join("\n"));
    done.push("mod.rs: added `pub npm`");
  }
}

// ── 2. parse it, and fill all four struct literals ───────────────────────────
{
  const lines = readFileSync(PARSER, "utf8").split("\n");

  if (!lines.some((l) => l.includes('params.get("npm")'))) {
    const at = lines.findIndex((l) => l.includes('params.get("model")'));
    if (at === -1) throw new Error("model param read not found in parser.rs");
    lines.splice(at + 1, 0, '    let npm = params.get("npm").cloned();');
    done.push("parser.rs: read `npm` param");
  }

  // The provider literal carries `model,`; prompt/mcp/skill carry `model: None,`.
  // Walk backwards so the index shifts do not matter.
  const withValue = lines
    .map((l, i) => [l, i])
    .filter(([l]) => /^ {8}model,$/.test(l))
    .map(([, i]) => i);
  const withNone = lines
    .map((l, i) => [l, i])
    .filter(([l]) => /^ {8}model: None,$/.test(l))
    .map(([, i]) => i);

  if (withValue.length !== 1) {
    throw new Error(`expected 1 \`model,\` line, found ${withValue.length}`);
  }
  if (withNone.length !== 3) {
    throw new Error(`expected 3 \`model: None,\` lines, found ${withNone.length}`);
  }

  for (const i of [...withValue, ...withNone].sort((a, b) => b - a)) {
    const line = withValue.includes(i) ? "        npm," : "        npm: None,";
    lines.splice(i + 1, 0, line);
  }
  writeFileSync(PARSER, lines.join("\n"));
  done.push(`parser.rs: filled ${withValue.length + withNone.length} struct literals`);
}

// ── 3. honour both in the OpenCode builder ───────────────────────────────────
{
  let text = readFileSync(PROVIDER, "utf8");

  if (text.includes("AIMYRIA_OPENCODE_DEFAULT_MODEL")) {
    done.push("provider.rs: already honours npm/model");
  } else {
    const old = `    // Build models object
    let mut models = serde_json::Map::new();
    if let Some(model) = &request.model {
        models.insert(model.clone(), json!({ "name": model }));
    }

    // Default to openai-compatible npm package
    json!({
        "npm": "@ai-sdk/openai-compatible",
        "options": options,
        "models": models
    })
}`;

    if (!text.includes(old)) {
      throw new Error("build_opencode_settings body did not match");
    }

    const neu = `    // Build models object.
    //
    // A new provider ID cannot inherit an existing one's definition — the write
    // boundary rejects an empty \`models\`, which is why an OpenCode import used
    // to fail outright when the link carried no model. So the model is never
    // empty: the link's \`model\` wins, and the default below is what a one-click
    // link with no model falls back to.
    let mut models = serde_json::Map::new();
    let model = request
        .model
        .as_deref()
        .unwrap_or(AIMYRIA_OPENCODE_DEFAULT_MODEL);
    models.insert(model.to_string(), json!({ "name": model }));

    // npm: the link may override; otherwise the openai-compatible package, which
    // is what an OpenAI-shaped relay endpoint speaks.
    let npm = request.npm.as_deref().unwrap_or(AIMYRIA_OPENCODE_DEFAULT_NPM);

    json!({
        "npm": npm,
        "options": options,
        "models": models
    })
}

/// The npm package an OpenCode provider uses unless the deep link names another.
///
/// Not a product guess: \`@ai-sdk/openai-compatible\` is what this function already
/// hardcoded before the link was able to override it.
const AIMYRIA_OPENCODE_DEFAULT_NPM: &str = "@ai-sdk/openai-compatible";

/// The model a one-click link configures OpenCode with when it names none.
///
/// A choice made here so that one-click works without a website change — the
/// current links carry no model. It has to be a single constant because the whole
/// point of a default is that it is changed in one place.
const AIMYRIA_OPENCODE_DEFAULT_MODEL: &str = "gpt-5";
}`;

    writeFileSync(PROVIDER, text.replace(old, neu));
    done.push("provider.rs: npm + model defaults, both overridable");
  }
}

console.log(done.map((d) => `  ${d}`).join("\n"));