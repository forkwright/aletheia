# Ardent Leatherworks Technical Stack
*Last updated: 2026-02-04*

## Overview

E-commerce system for handmade leather goods. Built on Zoho One + Stripe + Cloudflare.

## Contact Info
- **Phone:** 512-844-7309 (T-Mobile, not public)
- **Email:** cody@ardentleatherworks.com (Zoho Mail)
- **Aliases:** admin@ (infra), contact@ (public) → route to cody@

---

## Infrastructure

### Website
| Component | Service | Status |
|-----------|---------|--------|
| Static Site | 11ty (Eleventy) | ✅ Active |
| Hosting | Cloudflare Pages | ✅ Active |
| Domain | ardentleatherworks.com | ✅ Active |
| Repository | github.com/forkwright/ardent-site | ✅ Active |

### Payment Processing
| Component | Service | Status |
|-----------|---------|--------|
| Checkout | Stripe Payment Links | ✅ Active |
| Tax Calculation | Stripe Tax (auto) | ✅ Active |
| Shipping Collection | Stripe (US only) | ✅ Active |
| Webhook | Cloudflare Worker | ✅ Active |

**Payment URLs:**
- Belt ($150): https://buy.stripe.com/cNi9AT1ZHfFDeNRdsh6Ri02 (includes belt size field)
- Valet Tray ($85): https://buy.stripe.com/aFafZhawdgJHbBF1Jz6Ri01

**Webhook:** https://ardent-stripe-webhook.ardent-hooks.workers.dev

---

## Zoho One Apps

### Active & Configured
| App | Org ID | Purpose | API Access |
|-----|--------|---------|------------|
| **Books** | 913265477 | Accounting, invoices, customers | ✅ Full |
| **Checkout** | 913265477 | Payment pages (backup) | ✅ Full |
| **Inventory** | 913265477 | Material tracking | ✅ Full |
| **Mail** | - | cody@ardentleatherworks.com | ✅ Full |
| **CRM** | - | Customer pipeline | ✅ Full |

### Email Folders
| Folder | ID | Purpose |
|--------|-----|---------|
| Orders | 2286354000000010005 | Stripe notifications |
| Suppliers | 2286354000000012007 | Vendor emails |
| Customers | 2286354000000010007 | Customer correspondence |
| Accounting | 2286354000000015001 | Invoices, receipts |
| Zoho | 2286354000000013003 | System notifications |

**Note:** Email filters require ZohoMail.rules.ALL scope — create manually in UI

### Active (Additional)
| App | Purpose |
|-----|---------|
| **Campaigns** | Newsletter / email marketing |

### Available but Not Configured
| App | Potential Use |
|-----|---------------|
| **Forms** | Order customization requests |
| **Analytics** | Business intelligence |
| **Projects** | Production tracking |
| **Cliq** | Internal notifications |

### Not Using
| App | Reason |
|-----|--------|
| **Desk** | No support tickets yet |
| **Sign** | No contracts needed |
| **People** | Solo operation |

---

## Products

### Current Catalog
| Product | Price | Stock | Stripe ID |
|---------|-------|-------|-----------|
| Heritage Belt | $150 | 5 | prod_TteJFpRQFTXEFb |
| Valet Tray | $85 | 5 | prod_TteJ5rlab4ADuH |

### Inventory (Materials)
| Item | Stock | SKU |
|------|-------|-----|
| Hermann Oak Harness 11-13oz | 20 sq ft | LEATHER-HO-OWH-11 |
| Solid Brass Buckle 1.5in | 10 pcs | HARDWARE-BB-BUCKLE-15 |
| Solid Brass Chicago Screws | 50 pcs | HARDWARE-BB-CHICAGO-38 |
| Fil au Chinois 332 Thread | 5 spools | THREAD-FAC-332-NAT |
| Beeswax Block | 8 oz | FINISH-BEESWAX |
| Neatsfoot Oil | 16 oz | FINISH-NEATSFOOT |

---

## Integrations

### Working
| From | To | Method | Purpose |
|------|-----|--------|---------|
| Stripe | Zoho Books | Webhook | Auto-create invoices |
| Stripe | Stock Tracking | Webhook | Decrement on sale |
| Cloudflare | GitHub | Pages Deploy | Auto-deploy on push |

### Planned
| From | To | Purpose |
|------|-----|---------|
| Stripe | Zoho Mail | Order confirmation emails |
| Stripe | Zoho CRM | Customer record creation |
| Inventory | Alerts | Low stock notifications |

---

## Credentials & Secrets

### Location
All secrets stored in `/mnt/ssd/moltbot/demiurge/.secrets/`

| File | Contents |
|------|----------|
| `zoho.env` | OAuth tokens (Pay, Checkout, Books, Inventory, Mail, CRM) |
| `stripe.env` | API keys (live) |

### Cloudflare Worker Secrets
- ZOHO_CLIENT_ID
- ZOHO_CLIENT_SECRET
- ZOHO_REFRESH_TOKEN
- ZOHO_BOOKS_ORG_ID
- STRIPE_SECRET_KEY
- STRIPE_WEBHOOK_SECRET

---

## Known Gaps

### High Priority
1. **Email automation** — No order confirmation emails sent yet
2. **CRM pipeline** — Not receiving order data automatically

### Medium Priority
1. **Sales receipts** — Module not enabled in Books
2. **Low stock alerts** — Not implemented
3. **Product images** — Not on product pages yet

### Low Priority
1. **Analytics dashboard** — No business metrics view
2. **Customer portal** — Self-service order tracking
3. **Multi-currency** — USD only currently

---

## Pain Points

### Zoho Checkout API
- **Issue:** Limited API access for configuration
- **Workaround:** Using Stripe Payment Links instead
- **Status:** Resolved by switching to Stripe

### Zoho Books Sales Receipts
- **Issue:** Module disabled by default
- **Workaround:** Using invoices + payments instead
- **Status:** Works, but extra steps

### Duplicate Organizations
- **Issue:** A2Z Tree Service org (913264118) leftover from early setup
- **Resolution:** Deleted via Zoho One Admin → Organizations (2026-02-04)
- **Keep:** Ardent Leatherworks LLC (913265477)
- **Status:** ✅ RESOLVED

### OAuth Scope Discovery
- **Issue:** Finding correct scope names is trial-and-error
- **Workaround:** Documented working scopes
- **Status:** Resolved

---

## API Scopes (Current Token)

*Updated: 2026-02-04 14:07*

```
ZohoPay.fullaccess.all
ZohoCheckout.fullaccess.all
ZohoBooks.fullaccess.all
ZohoInventory.fullaccess.all
ZohoMail.messages.ALL
ZohoMail.accounts.ALL
ZohoMail.folders.ALL
ZohoCRM.modules.ALL
ZohoCRM.settings.ALL
ZohoCRM.users.ALL
ZohoCRM.coql.READ
ZohoCRM.org.ALL
ZohoCRM.bulk.ALL
WorkDrive.files.ALL
WorkDrive.team.ALL
WorkDrive.workspace.ALL
WorkDrive.teamfolders.ALL
WorkDrive.teamfolders.READ
WorkDrive.teamfolders.CREATE
```

**Note:** WorkDrive scopes use `WorkDrive.` prefix (not `ZohoWorkDrive.`).

---

## Zoho WorkDrive

**Team ID:** `tkhnk9afccb0222cf48029aee6601bf7def74`
**General Workspace ID:** `8z28qc821a2fd58414d41be991d5eaf38830f`

**Planned Structure:**
```
Ardent Leatherworks/
├── 01-Legal-Formation/
├── 02-Financial-Records/
├── 03-Brand-Documentation/
├── 04-Knowledge-Base/
└── 05-Reports-Audits/
```

---

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                     CUSTOMER JOURNEY                         │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  ardentleatherworks.com (Cloudflare Pages)                  │
│  ├── /products/belt                                          │
│  ├── /products/valet-tray                                    │
│  └── /thanks                                                 │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  Stripe Payment Links                                        │
│  ├── Automatic tax (Texas 8.25%)                            │
│  ├── Shipping address collection                             │
│  └── Stock tracking (metadata)                               │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  Cloudflare Worker (Webhook)                                 │
│  ├── Receive checkout.session.completed                     │
│  ├── Create customer in Zoho Books                          │
│  ├── Create invoice + payment                                │
│  └── Decrement stock / disable link if sold out             │
└─────────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  Zoho One (org: 913265477)                                  │
│  ├── Books: Invoices, customers, payments                   │
│  ├── Inventory: Material tracking                            │
│  ├── CRM: Customer pipeline                                  │
│  ├── Mail: cody@ardentleatherworks.com                      │
│  └── Checkout: Backup payment pages                          │
└─────────────────────────────────────────────────────────────┘
```

---

## Maintenance Notes

### Token Refresh
Zoho OAuth tokens expire in 1 hour. Webhook auto-refreshes using refresh_token.

### Updating Products
1. Update in Stripe (price, description)
2. Update on website (rebuild via git push)
3. Zoho Inventory syncs via webhook

### Checking Stock
```bash
source /mnt/ssd/moltbot/demiurge/.secrets/stripe.env
curl -s "https://api.stripe.com/v1/products" -u "$STRIPE_SECRET_KEY:" | jq '.data[] | {name, stock: .metadata.stock}'
```

### Restocking
```bash
curl -X POST "https://api.stripe.com/v1/products/PRODUCT_ID" \
  -u "$STRIPE_SECRET_KEY:" \
  -d "metadata[stock]=10"
```
