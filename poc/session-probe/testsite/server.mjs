import { randomBytes } from "node:crypto";
import { createServer } from "node:http";

const PORT = 43118;
const LISTEN_HOSTS = ["::1", "127.0.0.1"];
const COOKIE_NAME = "FCP-session-probe";
const SESSION_COOKIE_ATTRIBUTES = "Path=/; HttpOnly; SameSite=Lax";
const SESSION_SET_COOKIE_HEADER_REDACTED =
  `${COOKIE_NAME}=<redacted>; ${SESSION_COOKIE_ATTRIBUTES}`;
const DUMMY_USERNAME = "probe-user";
const DUMMY_PASSWORD = "probe-password";
const ALLOWED_REQUEST_ORIGINS = new Set([
  "chrome-extension://dokhjkpkdknopgnjdmaogjhlelcaiigo",
  "http://localhost:43118",
  "http://127.0.0.1:43118",
]);
const MAX_BODY_BYTES = 4096;
const MAX_REQUEST_DIAGNOSTICS = 10;

const sessions = new Map();
const requestCookieDiagnostics = [];
let securityAlarmCount = 0;
let requestDiagnosticSequence = 0;

async function handleRequest(request, response) {
  const url = new URL(request.url ?? "/", "http://loopback.invalid");

  if (!originAllowed(request)) {
    sendJson(request, response, 403, { error: "origin_not_allowed" });
    return;
  }

  if (request.method === "OPTIONS") {
    writeCorsHeaders(request, response);
    response.writeHead(204);
    response.end();
    return;
  }

  try {
    if (request.method === "GET" && url.pathname === "/") {
      sendHtml(request, response, 200, renderHome(sessionState(request)));
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/health") {
      sendJson(request, response, 200, { status: "ok", activeSessionCount: sessions.size });
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/login") {
      recordCookieHeaderDiagnostic(request, url.pathname);
      const credentials = await readJson(request);
      if (
        credentials.username !== DUMMY_USERNAME ||
        credentials.password !== DUMMY_PASSWORD
      ) {
        securityAlarmCount += 1;
        sendJson(request, response, 401, { authenticated: false, error: "invalid_credentials" });
        return;
      }

      const sessionId = randomBytes(32).toString("base64url");
      sessions.set(sessionId, { createdAt: Date.now(), username: DUMMY_USERNAME });
      response.setHeader(
        "Set-Cookie",
        `${COOKIE_NAME}=${sessionId}; ${SESSION_COOKIE_ATTRIBUTES}`,
      );
      sendJson(request, response, 200, { authenticated: true, state: "authenticated" });
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/protected") {
      recordCookieHeaderDiagnostic(request, url.pathname);
      const state = sessionState(request);
      sendJson(request, response, 200, state);
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/logout") {
      const sessionId = readSessionId(request);
      const invalidated = sessionId === undefined ? false : sessions.delete(sessionId);
      response.setHeader(
        "Set-Cookie",
        `${COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`,
      );
      sendJson(request, response, 200, { invalidated, state: "logged_out" });
      return;
    }
    if (request.method === "GET" && url.pathname === "/api/diagnostics") {
      sendJson(request, response, 200, {
        activeSessionCount: sessions.size,
        securityAlarmCount,
        sessionSetCookieHeaderRedacted: SESSION_SET_COOKIE_HEADER_REDACTED,
        requestCookieDiagnostics: requestCookieDiagnostics.map((entry) => ({ ...entry })),
      });
      return;
    }
    if (request.method === "POST" && url.pathname === "/api/reset") {
      const clearedSessionCount = sessions.size;
      sessions.clear();
      requestCookieDiagnostics.length = 0;
      requestDiagnosticSequence = 0;
      response.setHeader(
        "Set-Cookie",
        `${COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`,
      );
      sendJson(request, response, 200, { clearedSessionCount });
      return;
    }

    sendJson(request, response, 404, { error: "not_found" });
  } catch (error) {
    sendJson(request, response, 400, { error: error instanceof Error ? error.message : String(error) });
  }
}

for (const host of LISTEN_HOSTS) {
  createServer(handleRequest).listen(PORT, host, () => {
    console.log(`Session probe listener: ${host.includes(":") ? `[${host}]` : host}:${PORT}`);
  });
}
console.log(`Test origins: http://localhost:${PORT} and http://127.0.0.1:${PORT}`);
console.log(`Dummy login: ${DUMMY_USERNAME} / ${DUMMY_PASSWORD}`);

function sessionState(request) {
  const sessionId = readSessionId(request);
  if (sessionId === undefined) {
    return { authenticated: false, state: "logged_out", reason: "missing_cookie" };
  }
  if (!sessions.has(sessionId)) {
    return { authenticated: false, state: "logged_out", reason: "invalid_session" };
  }
  return { authenticated: true, state: "authenticated" };
}

function readSessionId(request) {
  const cookieHeader = request.headers.cookie;
  if (cookieHeader === undefined) {
    return undefined;
  }
  for (const pair of cookieHeader.split(";")) {
    const separator = pair.indexOf("=");
    if (separator < 0) {
      continue;
    }
    const name = pair.slice(0, separator).trim();
    if (name === COOKIE_NAME) {
      return pair.slice(separator + 1).trim();
    }
  }
  return undefined;
}

function recordCookieHeaderDiagnostic(request, path) {
  const cookieHeader = request.headers.cookie;
  requestDiagnosticSequence += 1;
  requestCookieDiagnostics.push({
    sequence: requestDiagnosticSequence,
    method: request.method ?? "<unknown>",
    path,
    host: request.headers.host ?? "<absent>",
    cookieHeaderPresent: cookieHeader !== undefined,
    cookieNames: readCookieNames(cookieHeader),
  });
  if (requestCookieDiagnostics.length > MAX_REQUEST_DIAGNOSTICS) {
    requestCookieDiagnostics.splice(0, requestCookieDiagnostics.length - MAX_REQUEST_DIAGNOSTICS);
  }
}

function readCookieNames(cookieHeader) {
  if (cookieHeader === undefined || cookieHeader.trim() === "") {
    return [];
  }
  return cookieHeader
    .split(";")
    .map((pair) => {
      const separator = pair.indexOf("=");
      return (separator < 0 ? pair : pair.slice(0, separator)).trim();
    })
    .filter((name) => name.length > 0);
}

async function readJson(request) {
  const chunks = [];
  let length = 0;
  for await (const chunk of request) {
    length += chunk.length;
    if (length > MAX_BODY_BYTES) {
      throw new Error("request_body_too_large");
    }
    chunks.push(chunk);
  }
  const text = Buffer.concat(chunks).toString("utf8");
  return JSON.parse(text);
}

function originAllowed(request) {
  const origin = request.headers.origin;
  return origin === undefined || ALLOWED_REQUEST_ORIGINS.has(origin);
}

function writeCorsHeaders(request, response) {
  const origin = request.headers.origin;
  if (origin !== undefined && ALLOWED_REQUEST_ORIGINS.has(origin)) {
    response.setHeader("Access-Control-Allow-Origin", origin);
    response.setHeader("Access-Control-Allow-Credentials", "true");
    response.setHeader("Access-Control-Allow-Headers", "Content-Type");
    response.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
    response.setHeader("Vary", "Origin");
  }
}

function sendJson(request, response, status, body) {
  writeCorsHeaders(request, response);
  response.writeHead(status, {
    "Cache-Control": "no-store",
    "Content-Type": "application/json; charset=utf-8",
  });
  response.end(JSON.stringify(body));
}

function sendHtml(request, response, status, body) {
  writeCorsHeaders(request, response);
  response.writeHead(status, {
    "Cache-Control": "no-store",
    "Content-Type": "text/html; charset=utf-8",
  });
  response.end(body);
}

function renderHome(state) {
  const label = state.authenticated ? "authenticated" : `logged out (${state.reason})`;
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>FURSOY Session Probe Test App</title>
    <style>
      body { font: 16px system-ui, sans-serif; margin: 3rem; max-width: 44rem; }
      form { display: grid; gap: 0.75rem; max-width: 24rem; }
      label { display: grid; gap: 0.25rem; }
      input, button { box-sizing: border-box; font: inherit; padding: 0.55rem; }
      #login-result { min-height: 1.5rem; }
    </style>
  </head>
  <body>
    <main>
      <h1>FURSOY Session Probe Test App</h1>
      <p>Controlled loopback application; no real account or external service.</p>
      <p>Current request state: <strong>${label}</strong></p>
      <form id="login-form">
        <label>Dummy username <input name="username" value="${DUMMY_USERNAME}" required></label>
        <label>Dummy password <input name="password" type="password" value="${DUMMY_PASSWORD}" required></label>
        <button type="submit">Log in with dummy account</button>
      </form>
      <p id="login-result" role="status"></p>
    </main>
    <script>
      document.querySelector("#login-form").addEventListener("submit", async (event) => {
        event.preventDefault();
        const result = document.querySelector("#login-result");
        result.textContent = "Logging in...";
        const fields = new FormData(event.currentTarget);
        const response = await fetch("/api/login", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            username: fields.get("username"),
            password: fields.get("password"),
          }),
        });
        if (!response.ok) {
          result.textContent = "Login failed: HTTP " + response.status;
          return;
        }
        location.reload();
      });
    </script>
  </body>
</html>`;
}
