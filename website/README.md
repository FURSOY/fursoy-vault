# FURSOY Vault website

Official product website for [FURSOY Vault](https://github.com/FURSOY/fursoy-vault), built with React, Vinext and Cloudflare Workers. This repository is the **reference implementation** of the FURSOY Web Design Standard — see `FURSOY_WEB_GUIDELINES.md` in this directory.

## Local development

```bash
npm install
npm run dev
```

The development server is available at `http://localhost:3000`.

## Verification

```bash
npm run lint
npm test
```

`npm test` creates a production build and checks the server-rendered homepage and privacy policy.

## Cloudflare deployment

Authenticate once with `npx wrangler login`, then publish with:

```bash
npm run deploy:cloudflare
```

In Cloudflare, open **Workers & Pages → fursoy (the Vault website worker) → Settings → Domains & Routes** and add `vault.fursoy.com` as a custom domain. The apex `fursoy.com` serves the FURSOY brand hub; legacy Vault paths on the apex redirect here.

The public website contains no analytics, telemetry, form processing or database bindings.
