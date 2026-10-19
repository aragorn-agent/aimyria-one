import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import vm from "node:vm";

const SCRIPT = resolve(
  "D:/aimyria中转站/04-源码/frontend-local/aimyria-one-import.js",
);

interface Harness {
  open: (url: string) => void;
  closed: string[];
  build: (app: string, endpoint: string, models: string[]) => URLSearchParams;
}

/**
 * Execute the production userscript in an isolated JS context.
 *
 * The script is dependency-free plain JavaScript so it can be deployed to both
 * website skins without a build step. Running the real file (instead of
 * re-implementing its URL logic) is what makes this regression test meaningful.
 */
function loadHarness(): Harness {
  const source = readFileSync(SCRIPT, "utf8");
  const closed: string[] = [];
  const fakeElement = () => ({
    setAttribute: () => undefined,
    removeAttribute: () => undefined,
    appendChild: () => undefined,
    remove: () => undefined,
    addEventListener: () => undefined,
    querySelector: () => null,
    querySelectorAll: () => [],
    style: {},
    textContent: "",
  });
  const document = {
    querySelector: () => null,
    querySelectorAll: () => [],
    getElementById: () => null,
    head: { appendChild: () => undefined },
    documentElement: { appendChild: () => undefined },
    body: { appendChild: () => undefined },
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
    hasFocus: () => true,
    createElement: fakeElement,
  };
  const windowObj: Record<string, unknown> = {
    open: (url: string) => {
      closed.push(url);
      return null;
    },
    __AIMYRIA_ONE_BALANCE__: {
      url: "https://aimyria.com/api/v1/aimyria/one/balance",
      token: "balance-token",
    },
  };
  const context = {
    window: windowObj,
    document,
    location: { origin: "https://aimyria.com", pathname: "/keys" },
    localStorage: { getItem: () => null, setItem: () => undefined },
    sessionStorage: { getItem: () => null, setItem: () => undefined },
    URLSearchParams,
    fetch: () =>
      Promise.resolve({ ok: false, json: () => Promise.resolve(null) }),
    setTimeout: () => 0,
    clearTimeout: () => undefined,
    console,
  };
  vm.createContext(context);
  vm.runInContext(source, context);
  const testApi = windowObj.__aimyriaOneImportTest as {
    buildImportUrl: (
      params: URLSearchParams,
      app: string,
      group: { name: string; models: string[] },
    ) => string;
  };
  return {
    open: (url) => (windowObj.open as (u: string) => void)(url),
    closed,
    build: (app, endpoint, models) => {
      const url = testApi.buildImportUrl(
        new URLSearchParams({
          resource: "provider",
          app: "codex",
          name: "Aimyria",
          endpoint,
          apiKey: "sk-test",
        }),
        app,
        { name: "国模·稳定", models },
      );
      return new URLSearchParams(url.slice(url.indexOf("?") + 1));
    },
  };
}

describe("website one-click import script", () => {
  it("passes through non-provider links", () => {
    const h = loadHarness();
    h.open("https://example.com");
    expect(h.closed).toEqual(["https://example.com"]);
  });

  it("accepts a provider link without throwing", () => {
    const h = loadHarness();
    expect(() =>
      h.open(
        "ccswitch://v1/import?resource=provider&app=codex&name=Aimyria&endpoint=https%3A%2F%2Fapi.aimyria.com&apiKey=sk-test",
      ),
    ).not.toThrow();
    expect(h.closed).toEqual([]);
  });

  it("gives Claude Code the origin without /v1", () => {
    const h = loadHarness();
    const params = h.build("claude", "https://api.aimyria.com/v1", [
      "claude-sonnet-5-5",
      "claude-opus-5",
      "claude-haiku-4-5",
    ]);
    expect(params.get("app")).toBe("claude");
    expect(params.get("endpoint")).toBe("https://api.aimyria.com");
    expect(params.get("model")).toBe("claude-sonnet-5-5");
    expect(params.get("sonnetModel")).toBe("claude-sonnet-5-5");
    expect(params.get("opusModel")).toBe("claude-opus-5");
    expect(params.get("haikuModel")).toBe("claude-haiku-4-5");
  });

  it("gives Codex /v1 and carries every model plus the balance bridge", () => {
    const h = loadHarness();
    const params = h.build("codex", "https://api.aimyria.com", [
      "gpt-5.6",
      "gpt-6.1-sol",
    ]);
    expect(params.get("app")).toBe("codex");
    expect(params.get("endpoint")).toBe("https://api.aimyria.com/v1");
    expect(params.get("model")).toBe("gpt-6.1-sol");
    expect(params.get("models")).toBe("gpt-5.6,gpt-6.1-sol");
    expect(params.get("groupName")).toBe("国模·稳定");
    expect(params.get("balanceUrl")).toBe(
      "https://aimyria.com/api/v1/aimyria/one/balance",
    );
    expect(params.get("balanceToken")).toBe("balance-token");
  });

  it("prefers DeepSeek in a mixed Chinese-model group", () => {
    const h = loadHarness();
    const params = h.build("dsh", "https://api.aimyria.com", [
      "gpt-5.6",
      "deepseek-v4.1-flash",
      "glm-5.3",
    ]);
    expect(params.get("model")).toBe("deepseek-v4.1-flash");
    expect(params.get("endpoint")).toBe("https://api.aimyria.com/v1");
  });
});
