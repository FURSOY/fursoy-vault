import { createHash } from "node:crypto";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { basename } from "node:path";
import { zipSync } from "fflate";

const staticFiles = [
  "manifest.json",
  "monitor-icon.png",
  "assets/deneme.png",
  "assets/fursoy-vault-extension-icon.png",
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
if (typeof manifest.key !== "string") {
  throw new Error("manifest public key is missing");
}

const extensionIdAlphabet = "abcdefghijklmnop";
const publicKeyDigest = createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest();
const extensionId = Array.from(publicKeyDigest.subarray(0, 16), (byte) =>
  `${extensionIdAlphabet[byte >>> 4]}${extensionIdAlphabet[byte & 0x0f]}`
).join("");
const identityConsumers = [
  "../native-host/src/dispatcher.rs",
  "../native-host/install/register.ps1",
  "../native-host/install/release/install.ps1",
  "../tests/acceptance/native_handshake.py",
];
for (const path of identityConsumers) {
  if (!(await readFile(path, "utf8")).includes(extensionId)) {
    throw new Error(`${path} does not authorize manifest extension ID ${extensionId}`);
  }
}

const entries = {};
for (const path of staticFiles) entries[path] = new Uint8Array(await readFile(path));
for (const file of scriptFiles) entries[`dist/${file}`] = new Uint8Array(await readFile(`dist/${file}`));
entries["LICENSE"] = new Uint8Array(await readFile("../LICENSE"));
entries["SOURCE.txt"] = new Uint8Array(await readFile("../SOURCE.txt"));

await rm("target", { recursive: true, force: true });
await mkdir("target", { recursive: true });

async function writeArchive(archiveName, archiveEntries) {
  const archivePath = `target/${archiveName}`;
  const archive = zipSync(archiveEntries, { level: 9, mtime: new Date("1980-01-01T00:00:00Z") });
  await writeFile(archivePath, archive);
  const sha256 = createHash("sha256").update(archive).digest("hex");
  await writeFile(`${archivePath}.sha256`, `${sha256}  ${basename(archivePath)}\n`, "utf8");
  console.log(`${archivePath}\nsha256 ${sha256}`);
}

await writeArchive(`fursoy-vault-extension-v${manifest.version}.zip`, entries);

// Chrome Web Store assigns the item ID from its own public key and rejects a manifest "key".
// Keep the key in the source package so unpacked development uses the production ID, but strip it
// from the dedicated upload artifact.
const storeManifest = { ...manifest };
delete storeManifest.key;
const storeEntries = {
  ...entries,
  "manifest.json": new TextEncoder().encode(`${JSON.stringify(storeManifest, null, 2)}\n`),
};
await writeArchive(`fursoy-vault-extension-v${manifest.version}-chrome-web-store.zip`, storeEntries);
