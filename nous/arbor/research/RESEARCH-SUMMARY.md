# A2Z Tree Service - Research Summary

*Compiled: 2026-01-31*

This document consolidates all initial research for quick reference.

---

## 1. Current Site Assessment

**URL:** https://www.a2z409.com/
**Domain:** Google Domains (now Squarespace) — needs Cloudflare migration

### What Exists
- Basic single-page site
- Services list: Tree removal, stump removal, pruning, trimming, palm tree removal, commercial, residential, natural disaster support
- About section for Adam
- Community-focused messaging

### What's Missing
- Work portfolio / before-after photos
- Contact form
- Reviews/testimonials  
- Readers' Choice award badge
- Proper navigation
- Quote request system

---

## 2. Texas Business Requirements

**Full details:** `texas-business-requirements.md`

### Quick Reference
| Requirement | Cost | Priority |
|-------------|------|----------|
| LLC Formation | $308 state fee | HIGH — liability protection |
| General Liability Insurance | $5-15k/year | HIGH — required by most clients |
| Workers' Comp | Required | IF employees |
| ISA Certification | ~$300 exam | RECOMMENDED — credibility |
| EIN (Tax ID) | Free | REQUIRED |

### Key Insight
No state licensing required for tree services in Texas, but insurance and LLC are essential for legitimacy and protection.

---

## 3. Competitive Landscape

**Full details:** `competitor-websites.md`

### Best Practices from Competitors
1. **Design:** Clean, minimal, earth tones
2. **Trust signals:** Licensed, insured, years in business — prominent
3. **Content:** Service-specific pages with details
4. **Portfolio:** Before/after galleries
5. **Contact:** Service-specific quote forms
6. **Testimonials:** Real names and locations

### What to Avoid
- Information overload
- Generic messaging
- Basic contact forms only
- No photos of actual work
- Outdated design

### Recommended Features for A2Z
- Hero with clear value prop + CTA
- Service pages with pricing guidance
- Before/after photo gallery
- Readers' Choice badge
- Easy quote request form
- Emergency services callout
- Area served (Galveston + surrounding)

---

## 4. Readers' Choice Award

**Full details:** `readers-choice-2025.md`

### Confirmed
- 2025 Galveston County Daily News Readers' Choice awards happened
- 240,000+ votes, 200+ categories
- A2Z Tree has excellent local reviews on Nextdoor

### Needs Verification
- Specific category Adam won
- Need to contact Daily News or ask Adam directly

### Competitor Tree Services in Area
- Cortazo Tree Service
- J.A.B. Company Tree & Landscaping
- EBG Landscaping & Services
- Potenza Tree Service
- Boulets Tree Service

---

## 5. Logo Direction

From Kendall:
- **Name:** A-2-Z Tree (2 is smaller)
- **Font idea:** Metallica-like for A and Z
- **Detail:** Tree roots on tail of A and/or Z
- **Tagline:** "Serving Galveston's tree needs A to Z"
- **Requirements:** Website + embroidery friendly, eye-catching colors

---

## 6. Open Questions

| Question | Status | Action |
|----------|--------|--------|
| Readers' Choice category | Unverified | Ask Adam |
| Metis downloads content | Blocked | Need Cody access |
| Current business structure | Unknown | Ask Adam (sole prop? LLC?) |
| Current hosting | Unknown | Check site headers |
| Adam's work photos | Unknown | Request from Adam |

---

## 7. Demiurge Recommendations

From Ardent experience (see `demi-ardent-lessons.md`):

**Stack:** Eleventy + Cloudflare Pages + GitHub Actions (proven, simple)

**Key insight:** Content-first / voice-early / scope-guard triad
- Know what you're saying before building
- Establish tone before writing
- Start minimal, resist creep

**Guard rails for Adam:**
- No direct file editing — changes through Arbor
- Single source of truth in `_data/business.json`
- Preview before deploy
- Content validation before push

**Reference files on NAS:**
- `/mnt/nas/docker/ardent-docs/ardent-site/.eleventy.js`
- `/mnt/nas/docker/ardent-docs/ardent-site/.github/workflows/deploy.yml`
- `/mnt/nas/docker/ardent-docs/ardent-site/src/_includes/base.njk`

---

## 8. Phase Plan

### Phase 1: Foundation (Current)
- [x] Research complete
- [ ] Get Demi's Ardent lessons
- [ ] Review Metis downloads
- [ ] Logo concepts
- [ ] GitHub repo setup

### Phase 2: Website Build
- [ ] Cloudflare domain transfer
- [ ] Site structure/wireframes
- [ ] Content gathering (photos, copy)
- [ ] Development
- [ ] Testing + launch

### Phase 3: Business Systems
- [ ] Invoice/estimate templates
- [ ] Google Drive structure
- [ ] Expense tracking setup

### Phase 4: Adam's Agent
- [ ] Direct messaging interface
- [ ] Site update capabilities
- [ ] Order/finance tracking

---

*This summary will be updated as research continues.*
