import http from "node:http";
import crypto from "node:crypto";

const HOST = "localhost";
const PORT = 43119;
const COOKIE_NAME = "FCP-mvp-session";
const sessions = new Map();
const diagnostics = [];
let invalidSessionCount = 0;

const server = http.createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", `http://${HOST}:${PORT}`);
    if (request.method === "GET" && url.pathname === "/") return html(response);
    if (request.method === "POST" && url.pathname === "/api/login") return login(request, response);
    if (request.method === "GET" && url.pathname === "/api/protected") return protectedEndpoint(request, response);
    if (request.method === "POST" && url.pathname === "/api/logout") return logout(request, response);
    if (request.method === "POST" && url.pathname === "/api/reset") return reset(response);
    if (request.method === "GET" && url.pathname === "/api/diagnostics") return json(response, 200, { active_session_count: sessions.size, invalid_session_count: invalidSessionCount, recent_requests: diagnostics.slice(-20) });
    return json(response, 404, { error: "not_found" });
  } catch (error) {
    console.error(error);
    return json(response, 500, { error: "server_error" });
  }
});

server.listen(PORT, HOST, () => console.log(`FURSOY controlled session app: http://${HOST}:${PORT}`));

async function login(request, response) {
  const body = await readJson(request);
  record(request, "/api/login");
  if (body.username !== "mvp-user" || body.password !== "mvp-password") return json(response, 401, { error: "invalid_credentials" });
  const sessionId = crypto.randomBytes(32).toString("hex");
  sessions.set(sessionId, { createdAt: Date.now() });
  response.setHeader("Set-Cookie", `${COOKIE_NAME}=${sessionId}; Path=/; HttpOnly; SameSite=Lax`);
  return json(response, 200, { state: "authenticated" });
}

function protectedEndpoint(request, response) {
  const sessionId = cookieValue(request.headers.cookie, COOKIE_NAME);
  record(request, "/api/protected");
  if (sessionId !== undefined && sessions.has(sessionId)) return json(response, 200, { state: "authenticated" });
  if (sessionId !== undefined) invalidSessionCount += 1;
  return json(response, 401, { state: sessionId === undefined ? "logged_out" : "invalid_session" });
}

function logout(request, response) {
  const sessionId = cookieValue(request.headers.cookie, COOKIE_NAME);
  record(request, "/api/logout");
  if (sessionId !== undefined) sessions.delete(sessionId);
  response.setHeader("Set-Cookie", `${COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`);
  return json(response, 200, { state: "logged_out" });
}

function reset(response) { sessions.clear(); invalidSessionCount = 0; diagnostics.length = 0; response.setHeader("Set-Cookie", `${COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`); return json(response, 200, { reset: true }); }
function record(request, route) { diagnostics.push({ at: new Date().toISOString(), route, cookie_header_present: request.headers.cookie !== undefined, cookie_names: cookieNames(request.headers.cookie) }); if (diagnostics.length > 100) diagnostics.splice(0, diagnostics.length - 100); }
function cookieNames(header) { return (header ?? "").split(";").map((part) => part.trim().split("=", 1)[0]).filter(Boolean); }
function cookieValue(header, name) { for (const part of (header ?? "").split(";")) { const [key, ...rest] = part.trim().split("="); if (key === name) return rest.join("="); } return undefined; }
function readJson(request) { return new Promise((resolve, reject) => { let body = ""; request.setEncoding("utf8"); request.on("data", (chunk) => { body += chunk; if (body.length > 4096) reject(new Error("body_too_large")); }); request.on("end", () => { try { resolve(JSON.parse(body || "{}")); } catch (error) { reject(error); } }); request.on("error", reject); }); }
function json(response, status, body) { response.writeHead(status, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store", "X-Content-Type-Options": "nosniff" }); response.end(JSON.stringify(body)); }
function html(response) { response.writeHead(200, { "Content-Type": "text/html; charset=utf-8", "Cache-Control": "no-store", "Content-Security-Policy": "default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'" }); response.end(`<!doctype html><html><head><meta charset="utf-8"><title>FURSOY Controlled Session</title><style>body{font:16px system-ui;max-width:720px;margin:4rem auto;padding:0 1rem}button,input{font:inherit;padding:.55rem;margin:.25rem}pre{background:#eee;padding:1rem}</style></head><body><h1>Controlled Session App</h1><p>Dummy credentials only: <code>mvp-user</code> / <code>mvp-password</code></p><form id="login"><input name="username" value="mvp-user"><input name="password" type="password" value="mvp-password"><button>Login</button></form><button id="health">Check protected state</button><button id="logout">Server-side logout</button><pre id="out">ready</pre><script>const out=document.querySelector('#out'); async function call(path,options){const r=await fetch(path,{credentials:'include',cache:'no-store',...options});const b=await r.json();out.textContent=JSON.stringify({status:r.status,...b},null,2)} document.querySelector('#login').onsubmit=(e)=>{e.preventDefault();const f=new FormData(e.target);call('/api/login',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(Object.fromEntries(f))})};document.querySelector('#health').onclick=()=>call('/api/protected');document.querySelector('#logout').onclick=()=>call('/api/logout',{method:'POST'});</script></body></html>`); }
