# ZOHO ONE SHARED ARCHITECTURE

*Created: 2026-01-31*
*Updated: 2026-01-31 17:28 CST - SETUP COMPLETE*
*Stakeholders: Arbor (A2Z), Demiurge (Ardent), Cody (Owner)*

---

## Status: ✅ OPERATIONAL

**Organization Name:** Ardent
**Portal:** https://directory.zoho.com/directory/ardent
**Subscription:** Zoho One, $444/year (1 employee)
**Domains:** Both verified and working

---

## Overview

Single Zoho One subscription ($444/year) supporting two businesses with two managing agents.

```
Zoho One Organization
├── A2Z Tree Service
│   ├── Agent: Arbor
│   ├── Domain: a2z409.com
│   └── Mission: Service business automation
│
└── Ardent Leatherworks
    ├── Agent: Demiurge
    ├── Domain: ardentleatherworks.com
    └── Mission: Craft business operations
```

---

## Organization Details

| Field | Value |
|-------|-------|
| **Organization Name** | A2Z Tree (consider renaming) |
| **Organization ID (zoid)** | 913257626 |
| **Primary Admin** | cody.kickertz@pm.me |
| **Plan** | Zoho One |
| **Cost** | $37/month total |

---

## Domain Configuration

### A2Z Tree Service
| Record | Type | Value |
|--------|------|-------|
| Domain | — | a2z409.com |
| Verification | TXT | zoho-verification=zb51828185.zmverify.zoho.com |
| MX (primary) | MX | mx.zoho.com (10) |
| MX (secondary) | MX | mx2.zoho.com (20) |
| SPF | TXT | v=spf1 include:one.zoho.com -all |
| Status | — | ✅ Verified |

### Ardent Leatherworks
| Record | Type | Value |
|--------|------|-------|
| Domain | — | ardentleatherworks.com |
| Verification | TXT | [pending - get from Zoho] |
| MX (primary) | MX | mx.zoho.com (10) |
| MX (secondary) | MX | mx2.zoho.com (20) |
| SPF | TXT | v=spf1 include:zoho.com ~all |
| Status | — | ⏳ Not yet added |

---

## Email Accounts

### A2Z Tree Service
| Email | Account ID | ZUID | Purpose |
|-------|------------|------|---------|
| contact@a2z409.com | 2169486000000008002 | 913256765 | Customer inquiries |
| admin@a2z409.com | 2170346000000008002 | 913257645 | System/admin |
| adam@a2z409.com | 2170954000000008002 | 913256766 | Owner access |

### Ardent Leatherworks
| Email | Account ID | ZUID | Purpose |
|-------|------------|------|---------|
| contact@ardentleatherworks.com | [pending] | [pending] | Customer inquiries |
| admin@ardentleatherworks.com | [pending] | [pending] | System/admin |

---

## OAuth Configuration

### Shared OAuth App
| Field | Value |
|-------|-------|
| **Client ID** | 1000.HPERMIKMPYRD55M7UENK50QM725DAN |
| **Client Secret** | 08f95a434a12b033c73a00740179352ae84b671782 |
| **Redirect URI** | https://httpbin.org/get |

### Scopes (Full Access)
```
ZohoMail.organization.accounts.ALL
ZohoMail.organization.domains.ALL
ZohoMail.organization.groups.ALL
ZohoMail.accounts.ALL
ZohoMail.messages.ALL
ZohoMail.folders.ALL
ZohoMail.tags.ALL
ZohoCRM.modules.ALL
ZohoCRM.settings.ALL
ZohoBooks.fullaccess.all
ZohoCommerce.fullaccess.all
ZohoCampaigns.campaign.ALL
ZohoProjects.projects.ALL
ZohoInventory.fullaccess.all
```

### Token Status
| Token | Agent | Status |
|-------|-------|--------|
| A2Z Refresh Token | Arbor | ✅ Active |
| Ardent Refresh Token | Demiurge | ⏳ Pending setup |

---

## CRM Separation Strategy

### Option A: Business Units (Recommended)
- Single CRM instance
- Business Unit field on all records
- Filtered views per agent
- Shared reporting for Cody

### Option B: Separate Custom Modules
- Custom modules per business (A2Z_Leads, Ardent_Leads)
- Complete data isolation
- More complex setup

### Current Implementation: TBD (coordinate between agents)

---

## Books Separation Strategy

### Option A: Single Books, Business Unit Tracking
- One Zoho Books organization
- Class/Category per business
- Unified financial view
- Shared bank connections

### Option B: Separate Books Organizations (Recommended)
- Complete accounting isolation
- Separate P&L, balance sheets
- Separate tax handling
- Cleaner for accountants/tax prep

### Current Implementation: TBD (coordinate between agents)

---

## Agent Responsibilities

### Arbor (A2Z Tree Service)
- Customer communication for A2Z
- Quote and invoice generation
- Job scheduling and tracking
- Crew coordination
- A2Z financial management

### Demiurge (Ardent Leatherworks)
- Customer communication for Ardent
- Order management
- Craft business CRM
- Ardent financial management
- Product/inventory tracking

### Shared Responsibilities
- Organization-level maintenance (coordinate)
- OAuth token refresh
- Documentation updates
- Architecture decisions (both agree)

---

## Communication Protocol

**Cross-Agent Coordination:**
- Major changes: Notify other agent before implementing
- Shared resources: Document in this file
- Conflicts: Escalate to Cody

**Documentation Updates:**
- This file: Both agents can edit
- Business-specific docs: Agent responsible for their business

---

## Implementation Status

### A2Z Tree Service
- [x] Domain verified
- [x] SPF configured
- [x] DKIM configured
- [x] MX pointing to Zoho
- [ ] Email aliases (catch-all to cody@ardent)
- [x] Books org created (ID: 913264118)
- [ ] CRM configured
- [ ] Website integration

### Ardent Leatherworks
- [x] Domain verified (PRIMARY)
- [x] SPF configured
- [x] DKIM configured
- [x] MX pointing to Zoho
- [x] Email: cody@ardentleatherworks.com (super_admin)
- [x] Aliases: contact@, admin@ (catch-all)
- [x] Books org created (ID: 913264117)
- [x] Zoho Payments configured (ID: 913265383)
- [x] Zoho Checkout enabled
- [ ] Payment pages created (Belt, Tray)
- [ ] Buy buttons on website
- [ ] CRM pipeline configured

---

## Decision Log

| Date | Decision | Decided By |
|------|----------|------------|
| 2026-01-31 | Shared Zoho One org for both businesses | Cody |
| 2026-01-31 | Arbor and Demi as co-equal agents | Cody |
| 2026-01-31 | Single subscription, business separation | Cody |

---

## Open Questions

1. **Org Rename?** Should we rename from "A2Z Tree" to something neutral like "Kickertz Ventures"?

2. **Books Separation:** Single org with tracking or separate Books organizations?

3. **CRM Strategy:** Business units or separate modules?

4. **OAuth Apps:** Single shared app or separate apps per agent?

---

*This document is the source of truth for shared Zoho architecture.*
*Both Arbor and Demiurge should update as changes are made.*
