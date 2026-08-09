import { createHash } from "node:crypto";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename } from "node:path";
import { zipSync } from "fflate";

const staticFiles = [
  "manifest.json",
  "monitor-icon.png",
  "theme.css",
  "popup.html", "popup.css",
  "options.html", "options.css",
  "unlock.html", "unlock.css",
  "onboarding.html", "onboarding.css",
];
const scriptFiles = [
  "background.js", "i18n.js", "locale.js", "monitor.js", "onboarding.js",
  "options.js", "popup.js", "protocol.js", "unlock.js",
];

const manifest = JSON.parse(await readFile("manifest.json", "utf8"));
if (manifest.manifest_version !== 3 || typeof manifest.version !== "string") {
  throw new Error("manifest version metadata is invalid");
}

const entries = {};
for (const path of staticFiles) entries[path] = new Uint8Array(await readFile(path));
for (const file of scriptFiles) entries[`dist/${file}`] = new Uint8Array(await readFile(`dist/${file}`));
entries["LICENSE"] = new Uint8Array(await readFile("../LICENSE"));
entries["SOURCE.txt"] = new Uint8Array(await readFile("../SOURCE.txt"));

await rm("target", { recursive: true, force: true });
await mkdir("target", { recursive: true });
const archiveName = `fursoy-vault-extension-v${manifest.version}.zip`;
const archivePath = `target/${archiveName}`;
const archive = zipSync(entries, { level: 9, mtime: new Date("1980-01-01T00:00:00Z") });
await writeFile(archivePath, archive);
const sha256 = createHash("sha256").update(archive).digest("hex");
await writeFile(`${archivePath}.sha256`, `${sha256}  ${basename(archivePath)}\n`, "utf8");
console.log(`${archivePath}\nsha256 ${sha256}`);
