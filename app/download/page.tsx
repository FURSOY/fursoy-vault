import type { Metadata } from "next";
import Image from "next/image";
import Link from "next/link";

const repository = "https://github.com/FURSOY/fursoy-vault";
const releases = `${repository}/releases`;
const downloadUrl = `${releases}/latest/download/FURSOY-Vault-Setup.exe`;

export const metadata: Metadata = {
  title: "Download",
  description: "Download the latest FURSOY Vault Windows companion from the project's official GitHub release.",
  alternates: { canonical: "/download" },
};

export default function Download() {
  return (
    <main className="legal-page download-page">
      <nav className="legal-nav"><Link href="/">Back to FURSOY Vault</Link><span>Official download</span></nav>
      <article>
        <Image className="download-icon" src="/app-icon.png" alt="FURSOY Vault icon" width={74} height={74} priority />
        <span className="kicker">Windows 10/11 · Google Chrome</span>
        <h1>Download FURSOY Vault.</h1>
        <p className="legal-lead">FURSOY Vault protects selected signed-in Chrome sessions by removing their cookies from the browser, storing them in an encrypted local Windows vault and restoring them only after explicit Windows Hello approval.</p>
        <div className="download-actions">
          <a className="button primary" href={downloadUrl}>Download Windows companion <span>↓</span></a>
          <a className="button secondary" href={releases}>Release history <span>↗</span></a>
        </div>
        <p className="download-note">The stable link always resolves to the Windows installer from the latest published GitHub release. The Chrome extension is installed separately.</p>

        <section><h2>What the installer contains</h2><p>The Setup executable installs the local Windows companion and its safe automatic updater. The companion communicates only with the FURSOY Vault Chrome extension through Chrome Native Messaging. Later companion updates can be applied automatically when no protected operation is active.</p></section>
        <section><h2>Requirements and installation</h2><p>Use a supported Windows 10/11 account with Windows Hello configured and Google Chrome installed. Download and run <strong>FURSOY-Vault-Setup.exe</strong>. Windows may show an Unknown publisher warning because the installer is currently unsigned. Uninstallation is available through Windows Installed apps; removing local vault data remains an explicit user choice.</p></section>
        <section><h2>Verify the download</h2><p>Each GitHub release publishes <strong>FURSOY-Vault-Setup.exe.sha256</strong> next to the installer. Compare that SHA-256 checksum before installation. The release page identifies the corresponding source tag and build procedure.</p><a className="legal-link" href={releases}>View releases and checksums</a></section>
        <section><h2>Code signing status</h2><p>The Windows companion installer is currently distributed without an Authenticode signature and may appear as <strong>Unknown publisher</strong> on Windows. Download it only from the official GitHub Releases page and compare the installer against the adjacent SHA-256 checksum before installation.</p><p>Every public package is built and tested by the project&apos;s GitHub Actions workflow from its matching source tag. If trusted code signing is introduced later, this page and the release policy will be updated before the first signed release.</p><a className="legal-link" href={`${repository}/blob/main/CODE_SIGNING_POLICY.md`}>Read the complete code signing policy</a></section>
        <section><h2>Privacy</h2><p>FURSOY Vault has no project-operated cloud account, analytics or telemetry service. Vault contents and protected-site configuration remain on the user&apos;s device. External download traffic is handled by GitHub.</p><Link className="legal-link" href="/privacy">Read the privacy policy</Link></section>
      </article>
      <footer className="legal-footer"><span>FURSOY Vault · GPL-3.0-only</span><a href={repository}>Source code</a></footer>
    </main>
  );
}
