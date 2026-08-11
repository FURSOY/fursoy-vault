import type { Metadata } from "next";

export const metadata: Metadata = { title: "Privacy Policy", description: "How FURSOY Vault handles local browser and security data." };

/* eslint-disable @next/next/no-html-link-for-pages */
export default function Privacy() {
  return <main className="legal-page"><nav className="legal-nav"><a href="/">← FURSOY Vault</a><span>Privacy policy</span></nav><article><span className="kicker">Effective August 11, 2026</span><h1>Privacy without fine print.</h1><p className="legal-lead">FURSOY Vault is designed to work locally. It does not operate an analytics, advertising, telemetry or cloud account service.</p>
    <section><h2>What the product processes</h2><p>The Chrome extension reads and removes cookies only for sites you explicitly choose to protect. Those cookie records are transferred through Chrome Native Messaging to the companion application and stored in an encrypted vault on your Windows device.</p></section>
    <section><h2>What does not leave your device</h2><p>Cookie values, protected-site configuration, Chrome profile identifiers, recovery records and local security audit events are not sent to FURSOY or any FURSOY-operated server. FURSOY Vault does not include product analytics or telemetry.</p></section>
    <section><h2>Windows Hello</h2><p>Windows Hello approval happens through Windows APIs on your device. The extension receives only the result required to continue the local restore operation. Biometric data is not available to the extension or companion.</p></section>
    <section><h2>Permissions and diagnostics</h2><p>Chrome host permission is requested per protected domain. Local audit records are security-focused, redact secrets such as cookie values and are retained for up to 90 days. They remain on the device unless you choose to share them while reporting an issue.</p></section>
    <section><h2>Website hosting and external services</h2><p>This website is delivered through Cloudflare, which may process standard network request information needed to serve and secure the site under its own privacy policy. The website does not set analytics or advertising cookies. It links to GitHub for source code, issue reporting and software releases; GitHub and the code-signing providers apply their own privacy policies when you visit their services.</p></section>
    <section><h2>Removal</h2><p>You can remove protected sites and local vault data through the product controls. Uninstalling and purging the companion removes locally stored FURSOY Vault data, subject to normal operating-system backup behavior.</p></section>
    <section><h2>Questions and changes</h2><p>Privacy questions and policy changes are handled publicly through the project repository. Material revisions will update the effective date on this page.</p><a className="legal-link" href="https://github.com/FURSOY/fursoy-vault/issues">Open a GitHub issue ↗</a></section>
  </article><footer className="legal-footer"><span>FURSOY Vault · GPL-3.0</span><a href="https://github.com/FURSOY/fursoy-vault/blob/main/PRIVACY.md">View policy source ↗</a></footer></main>;
}
