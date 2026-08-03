import { createReadStream } from "node:fs";
import { stat } from "node:fs/promises";
import { createServer } from "node:http";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HOST = "localhost";
const PORT = 43117;
const ROOT = fileURLToPath(new URL(".", import.meta.url));
const CONTENT_TYPES = new Map([
  [".html", "text/html; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
]);

createServer(async (request, response) => {
  try {
    const url = new URL(request.url ?? "/", `http://${HOST}:${PORT}`);
    const relativePath = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
    const filePath = normalize(join(ROOT, relativePath));
    if (!filePath.startsWith(ROOT)) {
      respond(response, 403, "Forbidden");
      return;
    }

    const info = await stat(filePath);
    if (!info.isFile()) {
      respond(response, 404, "Not found");
      return;
    }

    response.writeHead(200, {
      "Cache-Control": "no-store",
      "Content-Type": CONTENT_TYPES.get(extname(filePath)) ?? "application/octet-stream",
    });
    createReadStream(filePath).pipe(response);
  } catch {
    respond(response, 404, "Not found");
  }
}).listen(PORT, HOST, () => {
  console.log(`Cookie probe test site: http://localhost:${PORT}`);
});

function respond(response, status, body) {
  response.writeHead(status, { "Content-Type": "text/plain; charset=utf-8" });
  response.end(body);
}
