# Website & Content Decisions: Ardent Leatherworks

*Extracted from exported chat session January 29-30, 2026*

## Executive Summary

Complete development of Ardent Leatherworks website from initial concept to live site. Migration from Squarespace to custom static site, deployment on Cloudflare Pages, and comprehensive brand implementation. E-commerce integration TBD.

## 1. Website Structure Decisions

### Platform Choice
- **Initial**: Squarespace (abandoned for lack of control)
- **Final Decision**: Static site generator (Eleventy/11ty) hosted on Cloudflare Pages
- **Reasoning**: Full control over design, performance, and every detail aligns with craft philosophy

### Site Architecture
```
/ (Home)
├── /products/
│   └── /products/belt/
├── /materials/
├── /dyes/
├── /philosophy/
├── /contact/
├── /privacy/
├── /terms/
└── /journal/ (placeholder)
```

### Navigation Philosophy
- Minimal, intentional navigation
- Greek translations on hover for all nav items
- Home page removes philosophy link (rotating Greek words link there instead)

## 2. Page Content Discussions

### Homepage Strategy
- **Core Tagline**: "The Hand Remembers What The Mind Aims To Forget"
- **Supporting Line**: "Attention is a moral act"
- **Visual Elements**: Three rotating Greek words (χείρ, μνήμη, προσοχή) in dye colors
- **Philosophy**: Threshold/filter design - resonates or doesn't, no convincing

### Philosophy Page Structure
1. The Hand
2. Material Memory  
3. Mortality
4. Attention
5. Process
6. Imperfection
7. Why
8. The Language (Ancient Greek explanation)
9. The Maker (Cody's background)

### Product Page Evolution
- **Initial**: Single belt page
- **Final**: Template for future products (/products/belt/)
- **Content**: Construction details, materials story, sizing, care, cost breakdown
- **Transparency**: Full cost breakdown showing $19 materials + $81 labor/workshop + $10 shipping = $150

### Materials Page
- **Hermann Oak**: Full provenance story, why vegetable tanning matters
- **Wickett & Craig**: Added for keepers, equal treatment to Hermann Oak
- **Buckle Guy**: Solid brass details, removed defensive comparisons
- **Thread**: Fil au Chinois Lin Câblé (corrected from initial Ritza assumption)

### Dyes Page
- **Aima** (#581523) - cost of continuity
- **Thanatochromia** (#2C1B3A) - what death leaves behind  
- **Aporia** (#5C8E63) - between unresolvable truths
- **Added**: "Ingredients as History" section with material provenance
- **Visual**: Each dye entry has gradient background in its color

## 3. Photography Requirements

### Product Photography Placeholders
- Full belt view
- Detail shots: stitching, edge work, buckle attachment
- Scale reference with common objects
- Close-up of keeper detail
- Hardware details (buckle, Chicago screws)
- Edge treatment close-up

### Philosophy
- Film photography preferred (Canon P + Voigtlander 35mm)
- CineStill aesthetic: "melancholy that's not melancholic"
- Natural lighting, honest representation

## 4. Product Description Approach

### Voice & Tone
- **Principle**: No buzzwords, no SEO optimization for its own sake
- **Style**: Prose, not bullets. Truth without compression for compression's sake
- **Approach**: Process as proof, materials speak for themselves
- **No defensive language**: Don't compare to alternatives, just state what it is

### Belt Description Structure
1. Construction overview
2. Materials sourcing (specific suppliers)
3. Process details (hand saddle-stitched, floating keeper, etc.)
4. Care instructions (no conditioning needed - tallow stuffed)
5. Sizing information
6. Warranty: "Workmanship guaranteed. If it fails, we fix it."

## 5. Content That Was Written/Revised

### Created Pages
- Complete homepage with tagline and philosophy
- Full materials page with supplier stories
- Dyes page with philosophical framework and color theory
- Philosophy page with maker background
- Contact page with clear policies
- Privacy policy and Terms of Sale (legal compliance)

### Key Copy Elements
- **Tagline**: "The Hand Remembers What The Mind Aims To Forget"
- **Motto**: "Ἡ χείρ μιμνήσκεται, ἡ διάνοια ἐπιλανθάνεται" (The hand remembers. The mind forgets.)
- **Footer**: "Ardent Leatherworks · Ἡ χείρ μιμνήσκεται"
- **Warranty Language**: Simple, direct warranty statement
- **Material Descriptions**: Detailed provenance for Hermann Oak, Wickett & Craig, hardware

## 6. SEO & Marketing Content Decisions

### SEO Strategy
- **Primary**: Minimal SEO that doesn't sacrifice philosophy
- **Title Updates**: "Hermann Oak Leather Belt — Handmade in Texas | Ardent"
- **Meta Descriptions**: Honest, descriptive without keyword stuffing
- **Approach**: Quality content that naturally attracts right customers

### Marketing Philosophy
- **No traditional marketing copy**: No "heirloom quality" badges or claims
- **Transparency Over Marketing**: Full cost breakdown, honest material sourcing
- **Anti-SEO Stance**: Avoid buzzwords, let materials and process speak
- **Target Customer**: Someone like Cody - burned by marketing, seeking real quality

## 7. Technical Implementation

### Typography Stack
- **Display**: Cormorant Garamond (16th century, philosophical texts)
- **Body**: Spectral (bookish warmth, long reading)
- **Monospace**: IBM Plex Mono (craft precision)

### Color System
- **Background**: #F7F3E8 (archival cotton rag paper)
- **Accent Background**: #F0EBE0 (paper shadow)
- **Dye Colors**: Accurate hex codes for each natural dye
- **Approach**: Every color choice must be recursive in meaning

### Greek Integration
- Navigation items translate to Ancient Greek on hover
- Philosophy headers have Greek translations
- Dye names use Greek typography
- Motto in Greek and English

## 8. E-commerce Integration

### Platform Decision
- **Active**: Stripe Payment Links + Zoho One backend
- **Migrated from**: Ecwid (discontinued)

### Payment Processing
- **Checkout**: Stripe Payment Links (embedded on product pages)
- **Tax**: Stripe Tax (automatic)
- **Shipping**: Collected via Stripe, "shipping included" pricing
- **Backend**: Zoho Books (invoices), Zoho Inventory (stock), Zoho CRM (customers)
- **Webhook**: Cloudflare Worker handles Stripe → Zoho sync

**Full documentation:** `memory/ardent-stack.md`

### Product Setup
- **Single Product**: Ardent Belt (ID: 812721277)
- **Price**: $150 (includes shipping)
- **Variations**: Waist sizes 30-38 (dropdown selector)
- **Removed**: Dye color options (keeping natural only for launch)

## 9. Domain & Hosting

### Migration Path
- **From**: Squarespace hosting + domain
- **To**: Cloudflare Pages + Cloudflare domain registration
- **DNS**: Preserved ProtonMail email routing throughout
- **Process**: GitHub → Cloudflare Pages auto-deployment

### Email Setup
- **Customer-facing**: contact@ardentleatherworks.com
- **Backend/Admin**: admin@ardentleatherworks.com  
- **Provider**: ProtonMail with custom domain

## 10. Brand Guidelines Established

### Language Philosophy
- Ancient Greek for precision and unfamiliarity (creates pause, wonder)
- Dye names carry philosophical weight (Aima, Thanatochromia, Aporia)
- No exposition or "about" sections - let work speak

### Visual Identity
- Logo direction: Αχ (Ardent + χείρ/hand) with spiral element
- Paper texture background (archival quality)
- Warm, minimal aesthetic: "Dark academia meets Japanese workwear"
- Consistent spacing and typography hierarchy

### Supplier Philosophy
- Best quality per material category
- Heritage practices over modern when superior
- Direct relationships with makers when possible
- Material integrity (solid brass means solid brass)

## Key Success Metrics

1. **Philosophy Alignment**: Every decision traces back to core philosophy
2. **Technical Excellence**: Fast loading, clean code, industry best practices
3. **Honest Pricing**: Transparent cost breakdown builds trust
4. **Content Quality**: No marketing speak, pure truth about materials and process
5. **Customer Filtering**: Site attracts right customers, repels wrong ones

## Next Steps (Post-Launch)

1. Product photography with film camera
2. Logo finalization (pending Stable Diffusion access)  
3. Additional product lines (hand-dyed belts in limited batches)
4. Content creation for journal section
5. Customer testimonials and social proof integration

---

*This represents a complete philosophy-driven approach to e-commerce website development, prioritizing authentic craft values over conventional marketing tactics.*