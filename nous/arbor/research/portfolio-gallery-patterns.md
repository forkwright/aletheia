# Portfolio Gallery Best Practices for Service Business Websites

*Research compiled for tree service/landscaping business portfolio design*  
*Date: January 2025*

## 1. Before/After Image Layouts for Tree Work

### Split-Screen Layouts
- **50/50 Vertical Split**: Most common and effective for desktop
  - Before image on left, after on right
  - Consistent divider line (subtle gray or branded color)
  - Equal image sizing prevents visual bias

- **Horizontal Slider/Reveal**: Interactive comparison
  - Drag handle allows user to control reveal
  - Libraries: Twenty20, img-comparison-slider
  - Higher engagement but requires JS implementation

- **Sequential Stacked**: Mobile-first approach
  - Before image above, after below on mobile
  - Side-by-side on larger screens
  - Clear "Before" and "After" labels

### Tree Work Specific Considerations
- **Consistent Angles**: Same viewpoint for before/after shots
- **Similar Lighting**: Take photos at same time of day when possible
- **Wide Context**: Show property impact, not just tree close-ups
- **Safety Focus**: Highlight hazard removal (fallen branches, disease)
- **Property Enhancement**: Show increased curb appeal, open space

### Layout Patterns
```css
.before-after {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 2rem;
}

@media (max-width: 768px) {
  .before-after {
    grid-template-columns: 1fr;
    gap: 1rem;
  }
}
```

## 2. Image Optimization for Web

### Technical Implementation

#### Lazy Loading
- **Native lazy loading**: `loading="lazy"` attribute
- **Intersection Observer API**: For custom implementations
- **Libraries**: LozAd.js, Lozad, or Intersection Observer polyfill
- **Above-fold exception**: Don't lazy load hero images

#### Responsive Images
```html
<img 
  src="tree-removal-800w.webp" 
  srcset="tree-removal-400w.webp 400w,
          tree-removal-800w.webp 800w,
          tree-removal-1200w.webp 1200w"
  sizes="(max-width: 768px) 100vw, 50vw"
  loading="lazy"
  alt="Oak tree removal - before and after"
/>
```

#### Format Strategy
1. **WebP first**: 25-35% smaller than JPEG
2. **AVIF fallback**: Even better compression when supported
3. **JPEG backup**: Universal support
4. **Progressive JPEG**: Better perceived performance

#### Optimization Targets
- **Desktop**: 1200px max width, 80-85% quality
- **Mobile**: 800px max width, 75-80% quality  
- **Thumbnails**: 400px max width, 70% quality
- **File size**: Target <200KB per image, <100KB for thumbnails

### Performance Best Practices
- **CDN delivery**: CloudFlare, AWS CloudFront
- **Next-gen formats**: Serve WebP with JPEG fallback
- **Preload critical images**: Hero/above-fold images
- **Optimize metadata**: Remove EXIF data to reduce file size

## 3. Organization: Service Type vs Chronological

### Service Type Organization (Recommended)
**Advantages:**
- Visitors find relevant work quickly
- Demonstrates expertise depth in each area
- Better for SEO (service-specific landing pages)
- Easier to update/maintain categories

**Structure Example:**
```
├── Tree Removal
│   ├── Emergency Removal
│   ├── Dead Tree Removal
│   └── Property Development
├── Tree Trimming/Pruning
│   ├── Crown Reduction
│   ├── Health Pruning
│   └── Aesthetic Shaping
├── Stump Grinding
├── Land Clearing
└── Storm Damage Response
```

### Hybrid Approach (Best of Both)
- **Primary navigation**: By service type
- **Secondary filter**: "Recent Work" or "This Year"
- **Featured section**: "Latest Projects" on homepage
- **Project dates**: Visible but not primary organization

### Implementation Strategy
- **URL structure**: `/portfolio/tree-removal/`, `/portfolio/pruning/`
- **Breadcrumbs**: Home > Portfolio > Tree Removal > Project Name
- **Cross-referencing**: Tag projects with multiple services
- **Search/filter**: Allow visitors to filter by service, date, property type

## 4. Mobile Gallery UX Patterns

### Touch-Optimized Interactions
- **Swipe navigation**: Horizontal swipe between images
- **Pinch-to-zoom**: Enable for detail viewing
- **Tap targets**: Minimum 44px touch targets
- **Loading states**: Skeleton screens or progressive loading

### Mobile-First Layouts

#### Card-Based Grid
```css
.portfolio-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 1rem;
  padding: 1rem;
}
```

#### Stacked Layout
- Single column on mobile (<768px)
- Two columns on tablet (768-1024px)
- Three+ columns on desktop (>1024px)

### Mobile Navigation Patterns
- **Bottom tab bar**: Easy thumb access
- **Sticky category filters**: Stay visible while scrolling
- **Back to top button**: Appears after scrolling
- **Breadcrumb trail**: Compressed for mobile

### Performance on Mobile
- **Smaller images first**: Load mobile-optimized versions
- **Progressive enhancement**: Basic functionality without JS
- **Offline capability**: Cache recently viewed images
- **Data-conscious**: Option to load high-res on demand

## 5. Captioning Without Being Wordy

### Formula for Tree Work Captions
**Structure**: Service + Location + Key Detail + Result
- "Oak removal, residential backyard, storm damage cleanup"
- "Crown reduction, commercial property, improved clearance and safety"
- "Stump grinding, front yard, landscape preparation"

### Effective Caption Patterns

#### Action-Focused (4-6 words)
- "Emergency oak removal after storm"
- "Pruned maples for power line clearance" 
- "Dead ash trees safely removed"

#### Problem-Solution (8-10 words)
- "Removed diseased elm threatening neighbor's roof structure"
- "Pruned overgrown hedges blocking customer's scenic view"

#### Technical + Benefit (6-8 words)
- "Crown thinning improved tree health, reduced wind resistance"
- "Selective pruning enhanced property curb appeal significantly"

### Caption Writing Guidelines

#### Do:
- **Start with action verb**: Removed, pruned, cleared, ground
- **Include tree type**: Oak, maple, pine (when relevant)
- **Mention key benefit**: Safety, aesthetics, health, clearance
- **Use specific terms**: Crown reduction vs. "trimmed"

#### Don't:
- **Marketing speak**: "Amazing transformation," "incredible results"
- **Obvious statements**: "Before and after photos"
- **Technical jargon**: Excessive arboriculture terminology
- **Unnecessary adjectives**: "Beautiful," "stunning," "perfect"

### SEO-Friendly Captions
- **Include location**: City/neighborhood when relevant
- **Use service keywords**: Tree removal, pruning, stump grinding
- **Property type**: Residential, commercial, municipal
- **Seasonal context**: Storm damage, spring cleanup, fall preparation

## 6. Gallery Technical Implementation

### Modern Gallery Libraries
- **PhotoSwipe**: Touch-friendly, responsive, 0 dependencies
- **Lightbox2**: Lightweight, simple implementation
- **GLightbox**: Modern alternative, video support
- **Swiper.js**: Advanced touch slider with lazy loading

### Accessibility Considerations
- **Alt text**: Descriptive, includes service and outcome
- **Keyboard navigation**: Arrow keys, escape to close
- **Screen reader support**: Proper ARIA labels
- **Focus management**: Return focus after closing modal

### Loading Strategy
```javascript
// Lazy load with Intersection Observer
const imageObserver = new IntersectionObserver((entries, observer) => {
  entries.forEach(entry => {
    if (entry.isIntersecting) {
      const img = entry.target;
      img.src = img.dataset.src;
      img.classList.remove('lazy');
      observer.unobserve(img);
    }
  });
});

document.querySelectorAll('img[data-src]').forEach(img => {
  imageObserver.observe(img);
});
```

## 7. Conversion Optimization

### Call-to-Action Integration
- **Project-specific CTAs**: "Get estimate for similar work"
- **Service landing pages**: Each category leads to relevant service page
- **Contact forms**: Pre-populate service type from portfolio context
- **Phone number visibility**: Prominently displayed, click-to-call on mobile

### Trust Building Elements
- **Project dates**: Show recent, consistent work
- **Customer quotes**: Brief testimonials with project photos
- **Before photos**: Don't hide problems, show expertise solving them
- **Process transparency**: Brief description of approach/timeline

### Analytics and Testing
- **Image engagement**: Track which projects get most views
- **Category performance**: Monitor which services generate most inquiries
- **Mobile vs desktop**: Optimize for actual user behavior
- **Load time impact**: Monitor gallery performance on conversion rates

---

## Quick Implementation Checklist

### Essential Features
- [ ] Mobile-responsive grid layout
- [ ] Lazy loading implementation
- [ ] WebP images with JPEG fallback
- [ ] Service-based organization with filtering
- [ ] Touch-friendly navigation
- [ ] Clear, action-focused captions
- [ ] Fast loading (< 3 seconds)

### Advanced Features
- [ ] Before/after slider interactions
- [ ] Search functionality
- [ ] Category filtering with smooth transitions
- [ ] Social sharing buttons
- [ ] Related projects recommendations
- [ ] Integration with estimate request forms

### Performance Targets
- [ ] Core Web Vitals passing scores
- [ ] Images < 200KB each
- [ ] Initial page load < 2 seconds
- [ ] Mobile usability score > 95
- [ ] Accessibility score > 90

---

*This research compilation focuses on practical implementation for tree service businesses. Regular updates recommended as web standards and user expectations evolve.*