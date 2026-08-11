import type { Metadata } from "next";
import Image from "next/image";
import Link from "next/link";

const repository = "https://github.com/FURSOY/fursoy-vault";
const releases = `${repository}/releases`;
const downloadUrl = `${releases}/latest/download/fursoy-vault-windows.zip`;

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
        <p className="download-note">The stable link always resolves to the Windows package from the latest published GitHub release. The Chrome extension is installed separately.</p>

        <section><h2>What the package contains</h2><p>The ZIP contains the local Windows companion, installation and uninstallation scripts, license information and a source reference for the matching release. The companion communicates only with the FURSOY Vault Chrome extension through Chrome Native Messaging.</p></section>
        <section><h2>Requirements and installation</h2><p>Use a supported Windows 10/11 account with Windows Hello configured and Google Chrome installed. Extract the downloaded ZIP, review its included README, then run the provided installer. The package also includes an uninstaller; purging local vault data remains an explicit user choice.</p></section>
        <section><h2>Verify the download</h2><p>Each GitHub release publishes a SHA-256 checksum next to the Windows ZIP. Compare that checksum before installation. The release page identifies the corresponding source tag and build procedure.</p><a className="legal-link" href={releases}>View releases and checksums</a></section>
        <section><h2>Code signing policy</h2><p>Free code signing provided by SignPath.io, certificate by SignPath Foundation. Every signing request requires manual approval, and only artifacts built from the public project repository are eligible.</p><ul className="role-list"><li>Committers and reviewers: <a href="https://github.com/FURSOY">FURSOY</a></li><li>Approvers: <a href="https://github.com/FURSOY">FURSOY</a></li></ul><a className="legal-link" href={`${repository}/blob/main/CODE_SIGNING_POLICY.md`}>Read the complete code signing policy</a></section>
        <section><h2>Privacy</h2><p>FURSOY Vault has no project-operated cloud account, analytics or telemetry service. Vault contents and protected-site configuration remain on the user&apos;s device. External download traffic is handled by GitHub.</p><Link className="legal-link" href="/privacy">Read the privacy policy</Link></section>
      </article>
      <footer className="legal-footer"><span>FURSOY Vault · GPL-3.0-only</span><a href={repository}>Source code</a></footer>
    </main>
  );
}
