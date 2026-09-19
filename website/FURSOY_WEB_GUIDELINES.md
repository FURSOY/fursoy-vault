# FURSOY Web Design Standard

**Status:** Canonical source of truth for all FURSOY product websites.
**Reference implementation:** the FURSOY Vault website that lives next to this file (`app/globals.css`, `app/layout.tsx`, `app/page.tsx`).
**Applies to:** every public FURSOY website (currently `vault.fursoy.com` (Vault), `mail.fursoy.com` (Mail), `portfolyom.fursoy.com` (Fon Takip), `fursoy.com` (brand hub), and any future FURSOY site).

The goal is **not** identical sites. Each product may have its own identity, content and page
composition. The goal is that every FURSOY site is recognizably part of the same product family.

This document is written for humans **and** AI assistants. If you are an AI working inside a single
cloned repository and cannot read the reference implementation, follow the MUST rules below plus the
short fallback in that repository's `website/WEBSITE_RULES.md`.

---

## 0. Purpose & scope

- Section 1 (**MUST**) lists the rules every FURSOY website must follow. These define the shared visual identity.
- Section 2 (**REFERENCE**) lists concrete values taken from the reference implementation. Adapt them freely to content and product needs; they are examples of the system in use, not requirements.
- Sections 3–10 cover surfaces, layouts, motion, SEO and conventions.
- Out of scope: application UIs (desktop/mobile app screens), dashboards, and non-public pages.
- Repository layout: when a repository contains product code **plus** its public website, the site lives in a `/website` directory (e.g. `fursoy-vault/website/`, `fursoy-mail/website/`). A repository that is **only** a website may keep the site at the repository root (e.g. the FURSOY brand hub). Do not nest a website-only repository under `/website`.

---

## 1. MUST — Canonical identity rules

### 1.1 Typography

| Rule | Value |
|---|---|
| Primary typeface | **Manrope** (variable weights; wordmarks/brand text at weight 800). |
| Label/kicker typeface | **Space Mono**, weight 700, **uppercase**, wide letter-spacing (`.1em`–`.13em`), sizes 8–11px. Used **only** for small labels: kickers, column headers, meta tags, chips — never for body copy or headings. |
| Heading character | Tight negative tracking (about `-.025em` to `-.065em`), near-solid line-height (`.99`–`1.08`), very heavy weight (750–800 for H1). |
| Body character | Small, light and airy: ~12–14px, line-height 1.7–1.9, muted color for supporting text. |
| Label ladder | Very small labels are part of the identity: 8–11px mono/semibold text is normal, not a defect. |
| Emphasis in headings | `<em>` inside an H1/H2 renders **not italic** but in the accent color (teal). |

### 1.2 Color family

| Token | Value | Role |
|---|---|---|
| `--ink` | `#0b3334` | Primary brand dark: text on light surfaces, primary buttons, dark section backgrounds. |
| `--teal` | `#13abb8` | Main accent: hover links, kicker text, highlights, decorative elements. |
| `--bright` | `#27d2d8` | Accent **on dark surfaces** (always the lightened variant on dark, never raw `--teal`). |
| `--paper` | `#f6f8f5` | Default page background on light surfaces. |
| `--line` | `#d9e4df` | Default 1px border color on light surfaces. |
| `--shadow` | `0 24px 80px rgba(8,47,49,.12)` | Standard elevation for floating cards. |

Dark surfaces are **shades of the ink family**, not gray or black: roughly `#082526` (footer) → `#0c292a` (cards) → `#0b3334`/`#0c3435` (sections). Accent on dark uses `--bright` or a lighter teal (`#6ce1dd`, `#69d9d5`). The paper/teal hue family must stay recognizable; a product may skew darker overall (see §3) but not introduce an unrelated palette (no purple, orange, red accents, etc.) without a product decision.

### 1.3 Layout system

| Rule | Value |
|---|---|
| Content container | `width: min(1180px, calc(100% - 40px)); margin-inline: auto;` — the ~1180px FURSOY layout. |
| Mobile gutter | ~26px (container becomes `min(100% - 26px, 1180px)` on small screens). |
| Narrow content | Legal/article content uses centered narrow containers: 760–850px. |
| Section rhythm | Generous: roughly 80–120px vertical padding per section (desktop ~120px, mobile ~80px). |
| Grid character | Airy multi-column grids with large gaps (55–100px); grids collapse to fewer columns at breakpoints. |

### 1.4 Geometry (border & radius)

| Rule | Value |
|---|---|
| Borders | Always thin: 1px solid, restrained. Light surfaces: `--line` (~`#d9e4df`). Dark surfaces: `#315252` family (~`#294647`, `#2a4849`). Nav divider may be `rgba(11,51,52,.1)`. |
| Radius family A (surfaces) | Large cards/surfaces: **17–26px** (18–22 typical, 26 for hero cards). |
| Radius family B (controls) | Small controls/buttons/inputs/chips: **7–13px** (11 typical). |
| Pills | Fully rounded: `999px` (nav CTA, badges, announcement chips). |
| Mixing rule | Do not mix the two families arbitrarily: surfaces get A, controls get B, pills stay 999px. |

### 1.5 Buttons

- Anatomy: inline-flex, `padding: ~14px 20px`, radius 11 (family B), `font-size: 13px`, `font-weight: 800`, icon/arrow gap ~14px.
- Primary/dark: solid `--ink` background, white text, soft dark shadow (`0 12px 30px rgba(11,51,52,.2)`); hover shifts to `#155051`.
- Secondary: 1px border (`#baccc7` family), translucent/white background, ink text.
- On accent/CTA surfaces: white button + ghost (semi-transparent border) button.
- Hover: `translateY(-2px)` with ~200ms transition. Arrow glyphs `↗` (external) and `↓` (download) are part of the button language.
- Nav CTA is a separate pattern: pill (999px), 1px border, teal hover — not the standard button.

### 1.6 Links

- Default: inherit color, **no underline**; hover → teal.
- Inline text links may use the underline pattern: `text-decoration: underline; text-underline-offset: ~5px`.
- Links on dark surfaces: lightened teal (`#69d9d5`, `#6bdcd7`).
- Legal/action links: small (12px), bold (700–800), teal family (`#078f9a` on light).

### 1.7 Kicker / label language

- A section opens with a mono kicker (700, 10px, uppercase, tracking `.13em`, teal family: `#0794a0` on light, `#6ce1dd` on dark) followed by a large tightly-tracked heading.
- Numbered labels (`01`–`06`), `●` status dots, and `＋`/`✓`/`×` glyph marks are recurring identity elements.
- Column headers in footers/cards use the same mono language at 8–10px.

### 1.8 Navbar & footer character

- **Navbar:** one bordered bar (~86px desktop, ~72px mobile) with brand at left (32px mark + wordmark; optional small mono tagline separated by a vertical border), text links center/right (13px, weight 650, ~34px gaps), pill CTA at right. Primary nav links hide below ~900px — the bar itself stays.
- **Footer:** dark ink-family surface (`#082526`), brand + link columns with **mono uppercase column headers** (9px, muted teal-gray), small light links (11px), and a bottom bar separated by a 1px dark border with the smallest text (8–9px).
- Legal/secondary pages may use a minimal centered nav (same brand logic, no full navbar).

### 1.9 Motion

- Minimal and functional: ~200ms transitions on color/transform; button lift on hover; smooth in-page scrolling.
- No large-scale animation frameworks, no autoplaying motion. Decorative depth comes from static radial glows and very slight rotations (~1deg), not animation.
- `prefers-reduced-motion: reduce` must disable smooth scroll and hover transforms.

### 1.10 Responsive approach

- Two practical breakpoints: ~900px (nav links hide, hero/column grids stack, section padding shrinks) and ~620px (mobile gutter, single-column grids, heading clamps reach their floor, card interiors stack).
- Grids collapse to fewer columns rather than introducing new layouts; content always reflows inside the same container system.

---

## 2. REFERENCE — Adaptable values

These values come from the reference implementation. They illustrate the system; adapt them to the page. Keep the *character* (scale, tightness, generosity), not the exact numbers.

- Heading clamps: H1 `clamp(48px, 6vw, 76px)` / lh `.99` / ls `-.065em`; H2 `clamp(34px, 4.5vw, 54px)` / lh `1.08` / ls `-.05em`; card H3 ~18px; legal H1 `clamp(48px, 8vw, 76px)`.
- Hero: `min-height: 710px`, two-column grid `.9fr / 1.1fr`, gap 55px.
- Section gaps: split-heading 80px, open-section 100px, FAQ 90px.
- Section padding-block: 120px desktop / 80px mobile.
- Grid ratios: `.9/1.1`, `1.4/.6`, `.75/1.25`, `1/1`, `.95/1.05`; card grids `repeat(3, 1fr)` / `repeat(4, 1fr)`.
- Derived muted-text ladder: `#506866` (lead) → `#647876` → `#6e817e` → `#71817f` (supporting).
- Accent support tones: chip background `#e5f8f6` + text `#07949d`; success `#177e83` on `#e8f8f6`; hover teal `#078c98`–`#155051`.
- Dark-surface borders: `#315252` (grid), `#294647` (inner), `#2a4849` (cards).
- Specific card compositions (product mock cards, terminal/code cards, split cards with `linear-gradient(145deg, #0c3435, #0c292a)` copy panel) are reference patterns, not required sections.

---

## 3. Dark & light surface strategy

- The system is a **light-first** identity (`--paper` base) that uses **dark ink-family surfaces as deliberate sections**: security/trust sections, product/code mockups, footers.
- A product site may present **dark-first** (e.g., FURSOY Mail) as long as: backgrounds are ink-family darks (not gray/black), text is near-white `#f4f6f8`-family, accents are the teal/bright family, and light-family tokens are still recognizable where used.
- Do not mix arbitrary grays: on light use the paper/line family; on dark use the ink-family ladder.
- There is currently **no automatic dark/light theme switching** (`prefers-color-scheme` is not implemented in the reference). A site picks its surface strategy deliberately.
- PWA/manifest colors follow the identity: `theme_color: #0B3334`, `background_color: #F6F8F5`.

---

## 4. Legal / article layout

Reference pattern for privacy, terms, download and similar pages (`.legal-page` in the reference):

- Centered narrow container (760–790px; nav may be slightly wider, ~850px).
- Minimal nav: back-brand left, page label right, bottom border.
- H1: large clamp, tight tracking; lead paragraph slightly larger than body (17–18px).
- Sections separated by `border-top: 1px solid var(--line)` with consistent top padding/margin (~39px in the reference).
- H2 ~20px, body 13px / lh 1.9, muted color.
- Action links: `.legal-link` pattern (small, bold, teal).
- Minimal dark footer strip (`.legal-footer`) rather than the full product footer.
- Download-style pages reuse the same article plus the standard button row.

---

## 5. Responsive behavior

- Follows MUST §1.10. Reference numbers: 900px and 620px breakpoints.
- At 900px: nav links hidden; hero and two-column grids stack; 3–4 column card grids drop to 2; architecture/step flows become vertical.
- At 620px: tighter gutter (26px); single-column grids; heading clamps at floor (H1 ~36–49px); product mock cards simplify; footer columns stack.
- Design new sections so they degrade by *collapsing* (columns → stack), not by hiding content.

---

## 6. Motion & accessibility

- See MUST §1.9. Concretely: `transition: .2s transform, .2s background` on buttons; `html { scroll-behavior: smooth }`; `@media (prefers-reduced-motion: reduce)` resets both.
- Interactive elements keep visible focus (browser default focus is acceptable; do not remove outlines without a replacement).
- Icon chips and status dots pair color with text/position — never color alone.

---

## 7. SEO & web standards

Every FURSOY site provides, for its **own canonical domain**:

- `metadataBase` + title template (`%s · <Product>`), description, keywords, canonical URL.
- Open Graph + Twitter card metadata with a 1200×630-family OG image and absolute URLs.
- JSON-LD `SoftwareApplication` (or appropriate type) with `downloadUrl`, `operatingSystem`, `license`, `offers`.
- `robots.txt` (allow all) pointing at its own sitemap; `sitemap.xml` listing public pages; web manifest with identity colors.
- Canonical/robots/sitemap/OG must all point at the same domain and change **together** when the domain changes.
- Internal links use root-relative paths where possible so the site is origin-portable.

---

## 8. Product & privacy conventions (not design identity)

> This section is a product/privacy convention, **not** part of the visual design system. It can be
> changed by a product decision without changing the design standard.

- FURSOY product sites currently ship **no analytics, no telemetry, no tracking cookies, no ads**.
- Do not add analytics or tracking to a FURSOY website without an explicit product decision from the owner.
- Keep external service mentions (Cloudflare, GitHub, Chrome Web Store) in privacy copy accurate when infrastructure changes.

---

## 9. Deviation rules

**May differ per product (encouraged):**
- Page composition, section order, number and type of sections.
- Surface strategy (light-first vs dark-first presentation).
- Content density, copy tone, imagery/product mockups.
- REFERENCE values from §2 adapted to content.
- Extra utility tokens derived from the canonical family (e.g., product-specific muted tones).

**Must not differ (MUST §1):**
- Typeface families (Manrope / Space Mono roles).
- The brand color family (ink/teal/bright/paper hues).
- Container logic (~1180px system), radius families, 1px border character.
- Button/link/nav/footer/kicker *behavior and character*.
- Motion philosophy (~200ms, minimal) and the two-breakpoint responsive approach.

When in doubt: match the reference implementation, or ask before inventing.

---

## 10. Reference implementation

- The FURSOY Vault website (this directory: `app/globals.css`, `app/layout.tsx`, `app/page.tsx`, `app/privacy/page.tsx`, `app/download/page.tsx`) is the living reference.
- Token precedence when implementing a new site: reuse the exact MUST token values; derive new tones only inside the ink/teal families; document any derived token in the site's own CSS.
- Each site keeps its own `website/WEBSITE_RULES.md` pointing back to this file with a short fallback summary.
