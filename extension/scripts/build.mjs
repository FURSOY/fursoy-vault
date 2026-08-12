import { rm } from "node:fs/promises";
import { build } from "esbuild";

const entryPoints = [
  "src/background.ts",
  "src/connection-readiness.ts",
  "src/cookie-chunks.ts",
  "src/i18n.ts",
  "src/guarded-removal.ts",
  "src/locale.ts",
  "src/monitor.ts",
  "src/operation-coordinator.ts",
  "src/onboarding.ts",
  "src/options.ts",
  "src/popup.ts",
  "src/protocol.ts",
  "src/site-permissions.ts",
  "src/state-machine.ts",
  "src/unlock.ts",
];

await rm("dist", { recursive: true, force: true });
await build({
  entryPoints,
  outdir: "dist",
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "chrome130",
  sourcemap: false,
  legalComments: "none",
  charset: "utf8",
  logLevel: "info",
});
