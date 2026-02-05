# ARDENT LEATHERWORKS — CONTACT INFO UPDATE PLAN

*Created: 2026-02-02*
*Current Stack: Zoho One + Stripe + Cloudflare Pages*

---

## EMAIL ARCHITECTURE

| Alias | Purpose | Routes To |
|-------|---------|-----------|
| **admin@ardentleatherworks.com** | Infrastructure, accounts, registrations | cody@ardentleatherworks.com |
| **contact@ardentleatherworks.com** | Suppliers, customer service, public | cody@ardentleatherworks.com |
| **cody@ardentleatherworks.com** | Primary inbox | Zoho Mail |

**Email Provider:** Zoho Mail (part of Zoho One)

---

## PHONE NUMBER

**Number:** 512-844-7309 (T-Mobile SIM via AGM M7)
**Use:** Business contact, supplier accounts, verifications
**NOT on website** — for accounts/suppliers only

---

## UPDATE CHECKLIST

### 1. WEBSITE — ardentleatherworks.com

| Item | Current | Update To |
|------|---------|-----------|
| Contact page email | contact@ardentleatherworks.com | ✓ Correct |
| Contact page phone | Not listed | Add T-Mobile? (your call) |

**Files:** `/src/contact.md`, `/src/_includes/base.njk`

---

### 2. ZOHO MAIL — Email Aliases ✅ DONE

| Alias | Status |
|-------|--------|
| admin@ardentleatherworks.com | ✅ Active, routes to cody@ |
| contact@ardentleatherworks.com | ✅ Active, routes to cody@ |

*Verified via API 2026-02-02*

---

### 3. STRIPE

**Dashboard:** dashboard.stripe.com

| Setting | Current | Update To |
|---------|---------|-----------|
| Account email | ? | admin@ardentleatherworks.com |
| Customer support email | ? | contact@ardentleatherworks.com |
| Phone | ? | T-Mobile number |
| Receipt reply-to | ? | contact@ardentleatherworks.com |

**Payment Links:**
- Belt: buy.stripe.com/cNi9AT1ZHfFDeNRdsh6Ri02
- Valet: buy.stripe.com/aFafZhawdgJHbBF1Jz6Ri01

---

### 4. ZOHO ONE APPS

#### Zoho Books (Org: 913265477) ✅ DONE
| Setting | Value |
|---------|-------|
| Business email | cody@ardentleatherworks.com ✓ |
| Phone | 512-844-7309 ✓ |

*Updated via API 2026-02-02*

#### Zoho CRM
| Setting | Update To |
|---------|-----------|
| Org email | admin@ardentleatherworks.com |
| Customer-facing sends | contact@ardentleatherworks.com |

#### Zoho Campaigns (if using for newsletter)
| Setting | Update To |
|---------|-----------|
| Sender email | contact@ardentleatherworks.com |
| Reply-to | contact@ardentleatherworks.com |

#### Zoho Inventory
| Setting | Update To |
|---------|-----------|
| Notifications | admin@ardentleatherworks.com |

---

### 5. CLOUDFLARE

**Dashboard:** dash.cloudflare.com → ardentleatherworks.com

| Setting | Update To |
|---------|-----------|
| Pages notifications | admin@ardentleatherworks.com |
| Security alerts | admin@ardentleatherworks.com |

**Note:** Email routing NOT needed — using Zoho Mail

---

### 6. SUPPLIERS — Use contact@ardentleatherworks.com + T-Mobile

#### Springfield Leather Co (Hermann Oak)
- Account email: contact@
- Phone: T-Mobile

#### Abbey England
- Order history: SW0043615
- Account email: contact@
- Phone: T-Mobile

#### Rocky Mountain Leather Supply
- Account email: contact@
- Phone: T-Mobile

#### Amazon Business
- Update to contact@ for order notifications

---

### 7. BUSINESS REGISTRATIONS — Use admin@ardentleatherworks.com

| Entity | Email | Phone |
|--------|-------|-------|
| Northwest Registered Agent | admin@ | T-Mobile |
| Texas SOS (LLC/DBA) | admin@ | T-Mobile |
| IRS (EIN) | admin@ | T-Mobile |

---

### 8. BANKING

#### Relay Financial
- Status: Pending (open after EIN)
- Email: admin@ardentleatherworks.com
- Phone: T-Mobile (for 2FA)

---

### 9. SOCIAL

#### Instagram — @ardentleatherworks
- Bio email: contact@ardentleatherworks.com
- Phone: Optional

---

### 10. DOMAIN REGISTRAR

**Domain:** ardentleatherworks.com
**Registrar:** Squarespace

- WHOIS contact: admin@ardentleatherworks.com
- Renewal notifications: admin@ardentleatherworks.com

---

### 11. GITHUB

**Repo:** github.com/forkwright/ardent-site

- Notifications: admin@ardentleatherworks.com (or keep personal)

---

## STATUS — ALL COMPLETE ✅

### Done via API
1. ✅ Zoho Mail — aliases exist (admin@, contact@ → cody@)
2. ✅ Zoho Books — phone updated (512-844-7309)

### Done manually by Cody (2026-02-02)
3. ✅ Website — contact@ showing correctly
4. ✅ Stripe — updated
5. ✅ Supplier accounts — updated
6. ✅ Instagram bio — updated
7. ✅ Northwest RA — updated
8. ✅ IRS/EIN — updated

### Not changing
- **Relay** — keeping as personal banking for easy access

---

## VERIFICATION TESTS

| Test | How |
|------|-----|
| admin@ receives | Send from external |
| contact@ receives | Send from external |
| Stripe receipts show contact@ | Test purchase |
| Zoho invoice shows contact@ | Generate test |
| Phone receives SMS | Verification code test |

---

## RESOLVED

1. **T-Mobile number:** 512-844-7309 ✓
2. **Phone on website:** NO ✓
3. **Zoho Campaigns:** YES, using for newsletter ✓
4. **Old Zoho org (913264117):** DELETED ✓

---

*Updated with actual current infrastructure: Zoho One + Stripe + Cloudflare Pages*
