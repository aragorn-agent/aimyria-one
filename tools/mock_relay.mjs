// A mock console that exercises the refresh contract.
//
// It exists to prove one property end to end, against real HTTP rather than a
// mocked transport: **a stale access token must be refreshed and the request
// replayed, without the caller seeing a 401** — and a refresh credential must be
// rotated exactly once even when several requests race.
//
// Run: node tools/mock_relay.mjs [port]
// Then point the app at it with AIMYRIA_RELAY_CONSOLE_BASE=http://127.0.0.1:<port>

import { createServer } from "node:http";

const port = Number(process.argv[2] ?? 8799);

// ── state ────────────────────────────────────────────────────────────────────
const state = {
  refreshToken: "rt-initial",
  accessToken: "at-1",
  accessCounter: 1,
  refreshCalls: 0,
  requests: [],
  // Flip to make the next refresh fail, simulating a revoked session.
  refreshReject: false,
  // Force the next access token to be considered expired.
  startExpired: true,
};

function json(response, status, body) {
  const payload = JSON.stringify(body);
  response.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(payload),
  });
  response.end(payload);
}

function readBody(request) {
  return new Promise((resolve) => {
    let raw = "";
    request.on("data", (chunk) => (raw += chunk));
    request.on("end", () => {
      try {
        resolve(raw ? JSON.parse(raw) : {});
      } catch {
        resolve({});
      }
    });
  });
}

const server = createServer(async (request, response) => {
  const url = new URL(request.url, `http://127.0.0.1:${port}`);
  const path = url.pathname;
  state.requests.push({ method: request.method, path });

  if (request.method === "GET" && path === "/__state") {
    return json(response, 200, {
      refreshCalls: state.refreshCalls,
      accessToken: state.accessToken,
      refreshToken: state.refreshToken,
      requests: state.requests,
    });
  }

  if (request.method === "POST" && path === "/__expire") {
    // Simulate the access token lapsing while the app is running.
    state.accessToken = "expired";
    return json(response, 200, { ok: true });
  }

  if (request.method === "POST" && path === "/__reject-refresh") {
    state.refreshReject = true;
    return json(response, 200, { ok: true });
  }

  if (request.method === "POST" && path === "/desktop/v1/auth/login") {
    const body = await readBody(request);
    if (body.account !== "user@example.com" || body.password !== "correct") {
      return json(response, 401, { error: "invalid_credentials" });
    }
    state.accessToken = "at-login";
    return json(response, 200, {
      accessToken: state.accessToken,
      accessTokenExpiresAt: Math.floor(Date.now() / 1000) + 900,
      refreshToken: state.refreshToken,
      refreshTokenExpiresAt: Math.floor(Date.now() / 1000) + 15_552_000,
    });
  }

  if (request.method === "POST" && path === "/desktop/v1/auth/refresh") {
    const body = await readBody(request);
    state.refreshCalls += 1;

    if (state.refreshReject) {
      return json(response, 401, { error: "session_revoked" });
    }
    if (body.refreshToken !== state.refreshToken) {
      // Reusing a rotated-away credential is exactly what the single-flight gate
      // must prevent. Report it loudly so the test can assert on it.
      return json(response, 401, { error: "refresh_reuse_detected" });
    }

    state.accessCounter += 1;
    state.accessToken = `at-${state.accessCounter}`;
    state.refreshToken = `rt-${state.accessCounter}`;
    return json(response, 200, {
      accessToken: state.accessToken,
      accessTokenExpiresAt: Math.floor(Date.now() / 1000) + 900,
      refreshToken: state.refreshToken,
      refreshTokenExpiresAt: Math.floor(Date.now() / 1000) + 15_552_000,
    });
  }

  if (path === "/desktop/v1/auth/me") {
    const auth = request.headers.authorization ?? "";
    const token = auth.replace(/^Bearer\s+/i, "");
    if (token !== state.accessToken) {
      return json(response, 401, { error: "access_token_invalid" });
    }
    return json(response, 200, { account: "user@example.com" });
  }

  if (path === "/desktop/v1/keys" && request.method === "GET") {
    const auth = request.headers.authorization ?? "";
    const token = auth.replace(/^Bearer\s+/i, "");
    if (token !== state.accessToken) {
      return json(response, 401, { error: "access_token_invalid" });
    }
    // Deliberately slow: gives concurrent callers time to pile up on the gate.
    await new Promise((resolve) => setTimeout(resolve, 50));
    return json(response, 200, [
      { id: "k1", name: "主力", keyMasked: "sk-…a324", status: "enabled" },
    ]);
  }

  return json(response, 404, { error: "not_found", path });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`mock console listening on http://127.0.0.1:${port}`);
  console.log("  login: user@example.com / correct");
  console.log("  controls: POST /__expire, POST /__reject-refresh, GET /__state");
});
