import assert from "node:assert/strict";
import test from "node:test";

async function render(path = "/") {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}-${path}`);
  const { default: worker } = await import(workerUrl.href);
  return worker.fetch(new Request(`http://localhost${path}`, { headers: { accept: "text/html" } }), { ASSETS: { fetch: async () => new Response("Not found", { status: 404 }) } }, { waitUntil() {}, passThroughOnException() {} });
}

test("renders the FURSOY Vault homepage", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html\b/i);
  const html = await response.text();
  assert.match(html, /FURSOY Vault/);
  assert.match(html, /Close the session/);
  assert.match(html, /Windows Hello/);
  assert.match(html, /Windows companion currently unsigned/);
  assert.match(html, /releases\/latest\/download\/FURSOY-Vault-Setup\.exe/);
  assert.match(html, /SoftwareApplication/);
  assert.match(html, /rel="canonical" href="https:\/\/fursoy\.com\/?"/);
  assert.doesNotMatch(html, /development preview|loading skeleton|Starter Project/i);
});

test("renders the privacy policy", async () => {
  const response = await render("/privacy");
  assert.equal(response.status, 200);
  const html = await response.text();
  assert.match(html, /Privacy without fine print/);
  assert.match(html, /does not include product analytics or telemetry/);
  assert.match(html, /Chrome Web Store User Data Policy/);
  assert.match(html, /Limited Use requirements/);
  assert.doesNotMatch(html, /SignPath|code-signing providers/);
});

test("serves search discovery files", async () => {
  const [robots, sitemap, manifest] = await Promise.all([
    render("/robots.txt"),
    render("/sitemap.xml"),
    render("/manifest.webmanifest"),
  ]);
  assert.equal(robots.status, 200);
  assert.match(await robots.text(), /Sitemap: https:\/\/fursoy\.com\/sitemap\.xml/);
  assert.equal(sitemap.status, 200);
  assert.match(await sitemap.text(), /https:\/\/fursoy\.com\/privacy/);
  assert.equal(manifest.status, 200);
  assert.match(await manifest.text(), /FURSOY Vault/);
});

test("renders the official download page and signing status", async () => {
  const response = await render("/download");
  assert.equal(response.status, 200);
  const html = await response.text();
  assert.match(html, /Download Windows companion/);
  assert.match(html, /FURSOY-Vault-Setup\.exe/);
  assert.match(html, /Code signing status/);
  assert.match(html, /Unknown publisher/);
  assert.match(html, /SHA-256 checksum/);
  assert.doesNotMatch(html, /SignPath/);
  assert.match(html, /blob\/main\/CODE_SIGNING_POLICY\.md/);
});
