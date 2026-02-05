# Ardent Lessons for Arbor

*From Demiurge, 2026-01-31*

## What Worked

**Stack:**
- **Eleventy (11ty)** - Static site generator, dead simple. One config file, markdown content, Nunjucks templates. No framework complexity.
- **Cloudflare Pages** - Free hosting, automatic deploys from GitHub, fast global CDN. Domain transfer from Google Domains to Cloudflare DNS is straightforward.
- **GitHub Actions** - Push to master, site rebuilds automatically. Zero manual deployment.

**Structure that proved essential:**
```
src/
  _includes/base.njk    # Single template, all pages extend it
  css/style.css         # One CSS file
  img/                  # Pass-through
  index.md              # Home
  products.md           # Index page
  products/*.md         # Individual product pages
  contact.md
  terms.md
  privacy.md
  404.md
```

**Key decisions:**
- Markdown for content, Nunjucks only for the base template
- Single CSS file (no Tailwind, no build complexity)
- No analytics, no tracking, no third-party JS
- Static over CMS - no database, no plugins, no security surface

## What I'd Do Differently

1. **Start with a content inventory first** - I iterated the philosophy/lexicon extensively. For a service business, nail down: services offered, service area, about/credentials, contact, and gallery structure BEFORE touching code.

2. **Establish voice early** - Spent significant effort calibrating tone. For Adam, figure out: professional but approachable? Technical expertise? What's his differentiator?

3. **Guard against scope creep** - Ardent grew a lexicon, journal, dye philosophy. A tree service needs: what you do, where you work, how to contact, portfolio of work. Start minimal.

## Guard Rails for Non-Technical Users

This is critical. Adam won't catch mistakes.

1. **No direct file editing** - Changes go through you/Arbor, not Adam touching markdown
2. **Content validation** - Before any push, verify: phone numbers render correctly, email links work, no broken images
3. **Single source of truth** - One place for business info (maybe a `_data/business.json`):
   ```json
   {
     "name": "A2Z Tree",
     "phone": "409-XXX-XXXX",
     "email": "adam@...",
     "serviceArea": "Galveston County",
     "license": "..."
   }
   ```
   Reference this everywhere. Change once, updates everywhere.
4. **Preview before deploy** - Run `npm start` locally, review, then push. Or set up a staging branch.
5. **Backup before changes** - Git handles this, but be explicit about reverting if something breaks.

## Technical Recommendations

**GitHub structure:**
```
arbor-site/           # or a2z-tree-site
├── .github/workflows/deploy.yml
├── .eleventy.js
├── package.json
├── src/
│   ├── _data/business.json
│   ├── _includes/base.njk
│   ├── css/style.css
│   ├── img/
│   ├── index.md
│   ├── services.md (or services/*.md for individual pages)
│   ├── gallery.md
│   ├── about.md
│   ├── contact.md
│   └── 404.md
```

**Hosting setup:**
1. Create GitHub repo (private or public)
2. Transfer domain to Cloudflare (or just point nameservers)
3. Create Cloudflare Pages project, connect to GitHub
4. Add secrets: `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`
5. Push - deploys automatically

**Contact form options:**
- Formspree (free tier, dead simple)
- Cloudflare Workers (if you want it in-ecosystem)
- Just email/phone link (simplest)

**For invoices/estimates later:**
- Wave (free invoicing)
- Square Invoices
- Don't build this into the site - use a service

## Files to Reference

**Ardent site:** `/mnt/nas/docker/ardent-docs/ardent-site/`
- `.eleventy.js` - minimal config example
- `package.json` - just eleventy, nothing else
- `.github/workflows/deploy.yml` - exact CI/CD setup
- `src/_includes/base.njk` - template structure
- `src/css/style.css` - can strip out Ardent-specific, keep structure

**Process doc:** `/mnt/nas/docker/ardent-docs/ardent-site/site-process.md` - internal decisions log

## Core Insight

> The philosophical depth of Ardent is Cody's domain. Adam's site should be clear, professional, trustworthy - not philosophical. Different audiences entirely. **Same stack, completely different content strategy.**

## Content-First / Voice-Early / Scope-Guard Triad

This is the real lesson:
1. **Content-first** — Know what you're saying before building
2. **Voice-early** — Establish tone before writing
3. **Scope-guard** — Start minimal, resist creep
