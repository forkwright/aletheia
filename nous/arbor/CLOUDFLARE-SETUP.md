# Cloudflare Setup Instructions for A2Z Tree Site

## Current Status ✅
- **GitHub Repository**: https://github.com/forkwright/a2z-tree-site
- **Site Structure**: Complete with Eleventy + current content migrated
- **Original Images**: Attempted to extract from current site (Google Sites protected)
- **Auth Code**: Domain unlocked in Squarespace, waiting for auth code

## Next Steps

### 1. Create Cloudflare Pages Project
1. Go to `dash.cloudflare.com` → Pages
2. Click "Connect to Git"
3. Connect GitHub account and select `forkwright/a2z-tree-site`
4. **Build settings:**
   - Framework preset: "None"
   - Build command: `npm run build`
   - Build output directory: `_site`
   - Root directory: (leave blank)
5. Click "Save and Deploy"

### 2. Get Cloudflare Nameservers  
1. In Cloudflare dashboard → "Add site"
2. Enter domain: `a2z409.com`
3. Choose "Free" plan
4. Cloudflare will scan existing DNS records
5. **Copy the 2 nameservers provided** (example format):
   ```
   dana.ns.cloudflare.com
   rex.ns.cloudflare.com
   ```

### 3. Update Domain Nameservers
**In Squarespace domain settings:**
1. Go to domain management for `a2z409.com`
2. Find "Name Servers" or "DNS Settings"  
3. Change from current nameservers to the Cloudflare nameservers
4. **Note**: Propagation takes 24-48 hours

### 4. Configure Custom Domain in Cloudflare Pages
1. Go back to your Pages project
2. Click "Custom domains" tab
3. Add both:
   - `a2z409.com`
   - `www.a2z409.com`
4. Cloudflare will automatically configure DNS records

### 5. Set Up GitHub Actions Secrets
For automatic deployment, add these secrets to the GitHub repo:
1. Go to `github.com/forkwright/a2z-tree-site` → Settings → Secrets
2. Add:
   - `CLOUDFLARE_API_TOKEN`: [Your API token]
   - `CLOUDFLARE_ACCOUNT_ID`: [From Cloudflare dashboard]

## Files Still Needed

### 1. Work Photos from Adam
- Current Google Sites images are protected
- Need Adam to provide:
  - Hero section image (tree work in action)
  - About section photo (Adam or team at work)  
  - Gallery images for future "Job Gallery" page

### 2. Award Details
- Need exact category Adam won in 2025 Galveston Readers' Choice
- Need award badge/logo from Daily News if available

## Site Features Ready

✅ **Mobile responsive design**
✅ **Professional layout based on competitor research**  
✅ **Current content migrated and improved**
✅ **Local SEO optimized (schema markup)**
✅ **Guard rails for Adam-proof updates**
✅ **Contact information prominently displayed**
✅ **Service listings from current site**

## Testing the Site

Once nameservers propagate:
1. Visit `a2z409.com` - should show new Eleventy site
2. Test mobile responsiveness
3. Verify contact information displays correctly
4. Check page load speeds (should be very fast on Cloudflare)

## Next Development Phase

Once live:
1. **Add Work Photos**: Replace placeholders with Adam's photos
2. **Award Badge**: Add Readers' Choice winner badge when details confirmed
3. **Contact Form**: Add Formspree integration for quote requests
4. **Gallery Page**: Expand "Job Gallery" with more work examples
5. **Business Pages**: Add service detail pages

---

**Domain Transfer Note**: The full domain transfer from Squarespace to Cloudflare can happen later with the auth code. For now, just changing nameservers gets the site live on Cloudflare's infrastructure.