import Image from "next/image";

const github = "https://github.com/FURSOY/fursoy-vault";
const releases = `${github}/releases`;
const downloadUrl = `${releases}/latest/download/FURSOY-Vault-Setup.exe`;
const webStoreUrl = "https://chromewebstore.google.com/detail/fursoy-vault/ibjddphkjppgkdbegjibddbjkagdlaea";
const sponsors = "https://github.com/sponsors/FURSOY";

const softwareApplicationSchema = {
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  name: "FURSOY Vault",
  description: "A local vault for Chromium browsers on Windows and Linux that restores protected session cookies only after you verify it is you.",
  applicationCategory: "SecurityApplication",
  operatingSystem: "Windows 10, Windows 11, Linux",
  softwareVersion: "0.5.6",
  downloadUrl,
  installUrl: downloadUrl,
  image: "https://vault.fursoy.com/app-icon.png",
  url: "https://vault.fursoy.com",
  sameAs: github,
  license: `${github}/blob/main/LICENSE`,
  offers: { "@type": "Offer", price: "0", priceCurrency: "USD", url: downloadUrl },
  featureList: [
    "Local encrypted vault",
    "Windows Hello or a TPM-held PIN",
    "Browser profile isolation",
    "Per-site host permissions",
    "No analytics or telemetry",
  ],
};

const Arrow = () => <span aria-hidden="true">↗</span>;

function Brand({ compact = false }: { compact?: boolean }) {
  return (
    <a className="brand" href="#top" aria-label="FURSOY Vault home">
      <Image src="/brand-mark.png" alt="" width={32} height={32} />
      <span>FURSOY <b>Vault</b></span>
      {!compact && <small>for Chrome on Windows &amp; Linux</small>}
    </a>
  );
}

function ProductPreview() {
  return (
    <div className="product-shell" aria-label="FURSOY Vault product preview">
      <div className="glow glow-one" /><div className="glow glow-two" />
      <div className="browser-card">
        <div className="browser-top"><div className="traffic"><i /><i /><i /></div><div className="address"><span>⌕</span> accounts.example.com</div><div className="browser-avatar">F</div></div>
        <div className="browser-body">
          <div className="browser-copy"><span className="eyebrow">Protected session</span><h3>Your session is sealed.</h3><p>The site’s cookies have left the browser and are encrypted in a vault on your own machine.</p><div className="locked-line"><span>●</span> Waiting for you to verify it’s you</div></div>
          <div className="vault-visual"><div className="vault-ring"><Image src="/brand-mark.png" alt="" width={106} height={106} /></div></div>
        </div>
      </div>
      <div className="extension-card">
        <div className="extension-head"><div className="extension-brand"><Image src="/app-icon.png" alt="" width={32} height={32} /><div><strong>FURSOY Vault</strong><span>Companion connected</span></div></div><span className="status-dot">Active</span></div>
        <div className="extension-stat"><div><span>Current site</span><strong>accounts.example.com</strong></div><span className="shield-pill">Protected</span></div>
        <div className="extension-row"><div className="site-letter">E</div><div><strong>example.com</strong><span>3 active cookies</span></div><button type="button">Lock now</button></div>
        <div className="extension-foot"><span>Profile · Personal</span><span>Open vault settings →</span></div>
      </div>
    </div>
  );
}

const securityCards = [
  ["01", "Local by design", "Session cookies are encrypted and stored on this device—not uploaded to a FURSOY account or cloud."],
  ["02", "Approval by you", "Restoring a session takes Windows Hello on Windows, or a PIN held by the TPM on Linux. Neither the gesture nor the PIN reaches the extension."],
  ["03", "Profile isolation", "Each browser profile gets its own vault identity, recovery view and protected-site scope."],
  ["04", "Fail-closed behavior", "If the companion, permission or integrity checks fail, the vault does not silently expose the session."],
  ["05", "Minimal site access", "Host permission is requested only for the domains you choose to protect, and can be revoked at any time."],
  ["06", "Inspectable, redacted audit", "Local diagnostics retain security events for 90 days while excluding cookie values and secrets."],
];

const faqs = [
  ["Does FURSOY Vault upload my cookies?", "No. Cookie values are processed locally between the browser extension and the companion app on your machine. The project has no analytics or telemetry service."],
  ["What happens when I lock a protected site?", "Its matching cookies are removed from the browser and stored in the encrypted local vault. Once you verify it is you, they are restored to that browser profile."],
  ["Does it protect every kind of browser data?", "No. FURSOY Vault protects selected cookies. It is not a password manager and does not currently vault localStorage, IndexedDB, downloads or browser history."],
  ["Which platforms are supported?", "Chromium browsers—Chrome, Edge, Brave and the rest—on Windows 10/11 or Linux, with the companion app installed. Windows needs a Windows Hello method on the account; Linux uses a PIN held by a TPM 2.0 chip."],
];

export default function Home() {
  return (
    <main id="top">
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(softwareApplicationSchema) }} />
      <nav className="nav wrap"><Brand compact /><div className="nav-links"><a href="#how">How it works</a><a href="#security">Security</a><a href="#scope">Scope</a><a href="#faq">FAQ</a></div><a className="nav-cta" href={github} target="_blank" rel="noreferrer">GitHub <Arrow /></a></nav>
      <section className="hero wrap">
        <div className="hero-copy"><div className="announcement"><span>●</span> Open source · Local-first · No telemetry</div><h1>Close the session.<br /><em>Keep the access.</em></h1><p className="hero-lead">FURSOY Vault removes selected session cookies from your browser, seals them in an encrypted vault on your own machine, and restores them only once you verify it is you.</p><div className="hero-actions"><a className="button primary" href={webStoreUrl} target="_blank" rel="noreferrer" aria-label="Get FURSOY Vault from the Chrome Web Store">Get the extension <span>↓</span></a><a className="button secondary" href={github} target="_blank" rel="noreferrer">Explore the source <Arrow /></a></div><p className="availability"><i /> Chromium browsers on Windows 10/11 and Linux</p></div>
        <ProductPreview />
      </section>
      <section className="trust-strip"><div className="wrap trust-grid"><div><strong>LOCAL</strong><span>No account. No cloud vault.</span></div><div><strong>EXPLICIT</strong><span>You choose every protected site.</span></div><div><strong>ISOLATED</strong><span>Separate identity per browser profile.</span></div><div><strong>OPEN</strong><span>GPL-3.0 source, publicly inspectable.</span></div></div></section>
      <section className="section wrap" id="how">
        <div className="section-heading split-heading"><div><span className="kicker">How it works</span><h2>A deliberate pause between<br />your browser and your session.</h2></div><p>Protection is simple enough to use every day, but intentionally requires you when a locked session returns.</p></div>
        <div className="steps"><article><span className="step-number">01</span><div className="step-icon">＋</div><h3>Choose a site</h3><p>Add a domain from the extension. FURSOY Vault asks your browser only for that site’s permission.</p></article><article><span className="step-number">02</span><div className="step-icon">⌁</div><h3>Seal its session</h3><p>Matching cookies leave the browser and enter the encrypted vault held by the companion app on your machine.</p></article><article><span className="step-number">03</span><div className="step-icon">✓</div><h3>Approve the return</h3><p>When you revisit, your face, fingerprint or PIN is checked before the session cookies come back.</p></article></div>
      </section>
      <section className="section security-section" id="security"><div className="wrap"><div className="section-heading centered"><span className="kicker">Security architecture</span><h2>Trust should come from boundaries,<br />not a promise.</h2><p>Every important boundary is visible in the design and documented in the repository.</p></div><div className="security-grid">{securityCards.map(([n, title, text]) => <article key={n}><span>{n}</span><h3>{title}</h3><p>{text}</p></article>)}</div><div className="architecture-line"><div><span>BROWSER PROFILE</span><b>Selected cookies</b></div><i>→</i><div className="active"><span>LOCAL COMPANION</span><b>Encrypted local vault</b></div><i>→</i><div><span>USER PRESENCE</span><b>Your approval</b></div></div></div></section>
      <section className="section wrap scope-section" id="scope"><div className="scope-card"><div className="scope-copy"><span className="kicker light">Honest scope</span><h2>One focused layer.<br />Not a magic shield.</h2><p>FURSOY Vault reduces the risk of an unattended, already-signed-in browser session. It does not claim to protect a compromised computer account or replace the security controls around it.</p><a href={`${github}/blob/main/docs/THREAT_MODEL.md`} target="_blank" rel="noreferrer">Read the complete threat model <Arrow /></a></div><div className="scope-lists"><div><h3><span>✓</span> Designed to protect</h3><ul><li>Selected browser session cookies</li><li>Unattended signed-in sessions</li><li>Separation between browser profiles</li><li>Local recovery with explicit ownership</li></ul></div><div><h3><span>×</span> Outside its boundary</h3><ul><li>Passwords and passkeys</li><li>localStorage and IndexedDB</li><li>Malware already running as you</li><li>Browser history and downloaded files</li></ul></div></div></div></section>
      <section className="section wrap open-section"><div className="open-copy"><span className="kicker">Open source</span><h2>Security you can inspect.</h2><p>The Chrome extension, Rust companion, protocol, threat model, release checks and test suites are public. There is no hidden service behind the product. It is built and maintained by one person, and sponsorship goes first to the Windows code-signing certificate that would remove the <em>Unknown publisher</em> warning from every install.</p><div className="open-actions"><a className="button dark" href={github} target="_blank" rel="noreferrer">Browse the repository <Arrow /></a><a href={sponsors} target="_blank" rel="noreferrer">Sponsor the project</a><a href={`${github}/blob/main/PRIVACY.md`} target="_blank" rel="noreferrer">Privacy policy</a></div></div><div className="code-card"><div className="code-head"><span><i /><i /><i /></span><b>security-boundaries.txt</b></div><pre><code><span>$</span> data_location        this_device_only{"\n"}<span>$</span> telemetry            disabled{"\n"}<span>$</span> profile_scope        isolated{"\n"}<span>$</span> restore_approval     windows_hello{"\n"}<span>$</span> failure_mode         fail_closed</code></pre><div className="verified">✓ Documented and testable</div></div></section>
      <section className="section faq-section wrap" id="faq"><div className="section-heading"><span className="kicker">Common questions</span><h2>Know what you are installing.</h2></div><div className="faq-list">{faqs.map(([q, a], i) => <details key={q} open={i === 0}><summary>{q}<span>＋</span></summary><p>{a}</p></details>)}</div></section>
      <section className="cta-section wrap"><div><Image src="/app-icon.png" alt="FURSOY Vault icon" width={66} height={66} /><span className="kicker light">Ready when you are</span><h2>Put your open sessions<br />behind your presence.</h2><p>Review the source, understand the boundary, then install FURSOY Vault for your browser.</p><div className="hero-actions"><a className="button white" href={webStoreUrl} target="_blank" rel="noreferrer" aria-label="Get FURSOY Vault from the Chrome Web Store">Get the extension <span>↓</span></a><a className="button ghost" href={github} target="_blank" rel="noreferrer">GitHub <Arrow /></a></div></div></section>
      <footer><div className="wrap footer-main"><Brand /><div className="footer-links"><div><b>Project</b><a href="/download">Download</a><a href={github}>Source code</a><a href={releases}>Releases</a><a href={`${github}/issues`}>Issues</a><a href={sponsors}>Sponsor</a></div><div><b>Trust</b><a href="/privacy">Privacy</a><a href={`${github}/blob/main/docs/THREAT_MODEL.md`}>Threat model</a><a href={`${github}/blob/main/CODE_SIGNING_POLICY.md`}>Code signing policy</a></div></div></div><div className="wrap footer-bottom"><span>© 2026 FURSOY. Licensed under GPL-3.0.</span><span>Companion app currently unsigned · Verify release checksums before installation.</span></div></footer>
    </main>
  );
}
