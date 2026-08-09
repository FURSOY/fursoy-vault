// Message table for the "en" locale. See tr.ts for which surface is migrated so far.
export const en: Record<string, string> = {
  "onboarding.welcome.title": "Welcome to FURSOY Vault",
  "onboarding.welcome.body":
    "It keeps your important accounts' cookies in a TPM-protected vault while you're not using " +
    "them. Open a protected site and confirm with Windows Hello to get the cookie back; close " +
    "the tab or lock your PC and it's automatically put back in the vault.",
  "onboarding.welcome.next": "Continue",

  "onboarding.install.title": "Install the companion app",
  "onboarding.install.body":
    "FURSOY Vault needs a small companion app installed on your PC to work. Download it below, " +
    "open the file, and double-click install.bat.",
  "onboarding.install.downloadButton": "Download",
  "onboarding.install.checkButton": "Check connection",
  "onboarding.install.waiting": "Waiting for the companion app…",
  "onboarding.install.connected": "Connected.",
  "onboarding.install.notConnected":
    "Not connected yet. Make sure you ran install.bat, then reload this tab and try again.",
  "onboarding.install.skip": "Skip for now",

  "onboarding.addsite.title": "Protect your first site",
  "onboarding.addsite.body": "Which site's session do you want to protect? (Email, banking, social media, etc.)",
  "onboarding.addsite.scopeLabel": "Domain",
  "onboarding.addsite.scopePlaceholder": "example.com",
  "onboarding.addsite.policyLabel": "Protection level",
  "onboarding.addsite.submit": "Protect",
  "onboarding.addsite.skip": "Skip for now",
  "onboarding.addsite.error.empty": "Domain can't be empty.",
  "onboarding.addsite.error.overlap": "This domain already overlaps with a protected site.",
  "onboarding.addsite.error.permission": "Chrome permission was denied; the site was not protected.",
  "onboarding.addsite.error.generic": "Couldn't add protection.",

  "onboarding.done.title": "You're all set",
  "onboarding.done.body":
    "FURSOY Vault is now running. Click the toolbar icon any time to add another site or change settings.",
  "onboarding.done.finish": "Finish",

  "policy.critical": "Critical — 5 min lease · 1 min idle · evicts instantly",
  "policy.balanced": "Balanced — 10 min lease · 5 min idle · 2 min grace",
  "policy.convenient": "Convenient — 30 min lease · 15 min idle · 5 min grace",
  "policy.monitor": "Monitor only — no cookie vaulting",
};
