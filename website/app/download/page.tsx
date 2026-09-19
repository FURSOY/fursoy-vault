import type { Metadata } from "next";
import Image from "next/image";
import Link from "next/link";

const repository = "https://github.com/FURSOY/fursoy-vault";
const releases = `${repository}/releases`;
const downloadUrl = `${releases}/latest/download/FURSOY-Vault-Setup.exe`;
const linuxDownloadUrl = `${releases}/latest/download/fursoy-vault-linux-x86_64.tar.gz`;

export const metadata: Metadata = {
  title: "Download",
  description: "Download the latest FURSOY Vault companion for Windows or Linux from the project's official GitHub release.",
  alternates: { canonical: "/download" },
};

export default function Download() {
  return (
    <main className="legal-page download-page">
      <nav className="legal-nav"><Link href="/">Back to FURSOY Vault</Link><span>Official download</span></nav>
      <article>
        <Image className="download-icon" src="/app-icon.png" alt="FURSOY Vault icon" width={74} height={74} priority />
        <span className="kicker">Windows 10/11 &amp; Linux · Chromium browsers</span>
        <h1>Download FURSOY Vault.</h1>
        <p className="legal-lead">FURSOY Vault protects selected signed-in browser sessions by removing their cookies from the browser, storing them in an encrypted vault on your own machine and restoring them only after you explicitly approve.</p>
        <div className="download-actions">
          <a className="button primary" href={downloadUrl}>Download for Windows <span>↓</span></a>
          <a className="button primary" href={linuxDownloadUrl}>Download for Linux <span>↓</span></a>
          <a className="button secondary" href={releases}>Release history <span>↗</span></a>
        </div>
        <p className="download-note">Each stable link always resolves to that platform&apos;s companion from the latest published GitHub release. The browser extension is installed separately, from the Chrome Web Store.</p>

        <section><h2>What the download contains</h2><p>The companion app that holds the vault. It communicates only with the FURSOY Vault extension, through the browser&apos;s Native Messaging channel, and never over the network. On Windows the Setup executable also installs a safe automatic updater, which applies later companion updates only when no protected operation is active; on Linux the companion is updated the way everything else on the system is, by the package manager.</p></section>
        <section><h2>Requirements and installation</h2><p>A Chromium browser — Chrome, Edge, Brave or similar — and a machine with a TPM 2.0 security chip, which most from the last several years have.</p><p><strong>Windows 10/11:</strong> the account needs a Windows Hello method configured. Run <strong>FURSOY-Vault-Setup.exe</strong>; Windows may show an Unknown publisher warning because the installer is currently unsigned. Uninstall through Installed apps.</p><p><strong>Linux:</strong> extract the archive and run the included <strong>register.sh</strong>. You choose a PIN the first time a vault opens, and the TPM holds it. Run <strong>unregister.sh</strong> to remove it.</p><p>On both, removing the vault data itself stays an explicit choice rather than something uninstalling does for you.</p></section>
        <section><h2>Verify the download</h2><p>Each GitHub release publishes a <strong>.sha256</strong> file next to every download. Compare that SHA-256 checksum before installing. The release page identifies the corresponding source tag and build procedure.</p><a className="legal-link" href={releases}>View releases and checksums</a></section>
        <section><h2>Code signing status</h2><p>The Windows companion installer is currently distributed without an Authenticode signature and may appear as <strong>Unknown publisher</strong>. Linux has no equivalent prompt, but the same checksum verification applies there. Download it only from the official GitHub Releases page and compare the installer against the adjacent SHA-256 checksum before installation.</p><p>Every public package is built and tested by the project&apos;s GitHub Actions workflow from its matching source tag. If trusted code signing is introduced later, this page and the release policy will be updated before the first signed release.</p><a className="legal-link" href={`${repository}/blob/main/CODE_SIGNING_POLICY.md`}>Read the complete code signing policy</a></section>
        <section><h2>Privacy</h2><p>FURSOY Vault has no project-operated cloud account, analytics or telemetry service. Vault contents and protected-site configuration remain on the user&apos;s device. External download traffic is handled by GitHub.</p><Link className="legal-link" href="/privacy">Read the privacy policy</Link></section>
      </article>
      <footer className="legal-footer"><span>FURSOY Vault · GPL-3.0-only</span><a href={repository}>Source code</a></footer>
    </main>
  );
}
