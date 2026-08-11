# FURSOY Vault website

Official product website for [FURSOY Vault](https://github.com/FURSOY/fursoy-vault), built with React, Vinext and Cloudflare Workers.

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

In Cloudflare, open **Workers & Pages → fursoy-vault-website → Settings → Domains & Routes**, add `fursoy.com` as a custom domain, then add `www.fursoy.com` or redirect it to the apex domain.

The public website contains no analytics, telemetry, form processing or database bindings.
