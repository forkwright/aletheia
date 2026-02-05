# Complete Cloudflare Setup Guide: Domain Transfer, DNS, and Pages Hosting

This comprehensive guide covers transferring domains from Google Domains/Squarespace to Cloudflare, setting up DNS for business websites, deploying sites via Cloudflare Pages from GitHub, understanding free tier limits, and configuring SSL/security.

## Table of Contents
1. [Prerequisites and Preparation](#1-prerequisites-and-preparation)
2. [Domain Transfer from Google Domains/Squarespace](#2-domain-transfer-from-google-domainssquarespace)
3. [DNS Configuration for Business Sites](#3-dns-configuration-for-business-sites)
4. [Cloudflare Pages Deployment from GitHub](#4-cloudflare-pages-deployment-from-github)
5. [Free Tier Limits and Paid Plan Considerations](#5-free-tier-limits-and-paid-plan-considerations)
6. [SSL/Security Setup](#6-sslsecurity-setup)
7. [Troubleshooting and Best Practices](#7-troubleshooting-and-best-practices)

---

## 1. Prerequisites and Preparation

### Before You Start
- **Cloudflare Account**: Create a free account at [cloudflare.com](https://cloudflare.com)
- **Domain Requirements**: Domain must be at least 60 days old and not recently transferred
- **Financial Institution**: Notify your bank about potential domain transfer charges to prevent fraud flags
- **Backup DNS Records**: Document your current DNS settings before starting

### Important Timing Considerations
- Domain transfers can take up to 5 days (but often complete in 24-48 hours)
- If domain expires within 15 days, renew it first
- DNSSEC must be disabled before transfer (we'll cover this)

---

## 2. Domain Transfer from Google Domains/Squarespace

### Step 1: Prepare Your Current Domain

#### For Google Domains (now Squarespace):
1. **Log in** to your domain registrar dashboard
2. **Disable Privacy Protection** temporarily (if enabled)
3. **Unlock Domain Transfer Lock**:
   - Look for "Transfer Lock" or "Registrar Lock" setting
   - Switch to "Unlocked" or "Off"
4. **Get Authorization Code**:
   - Find "Authorization code" or "EPP code" section
   - Generate and copy this code (you'll need it later)

#### For Squarespace Domains:
1. Go to **Settings → Domains**
2. Click your domain → **DNS Settings**
3. **Disable Transfer Protection**
4. **Get Transfer Authorization Code**

### Step 2: Add Domain to Cloudflare (Before Transfer)

1. **Log into Cloudflare Dashboard**
2. Click **"Add a Site"**
3. **Enter your domain name** and click "Add Site"
4. **Select Plan**: Choose "Free" for basic features
5. **Review DNS Records**: Cloudflare will scan and import existing records
6. **Update Nameservers**: 
   - Note the assigned Cloudflare nameservers (e.g., `bob.ns.cloudflare.com`, `dina.ns.cloudflare.com`)
   - Go back to your current registrar
   - Replace existing nameservers with Cloudflare nameservers
   - Wait for propagation (can take up to 24 hours)

### Step 3: Disable DNSSEC (Critical Step)

**Why this matters**: DNSSEC must be disabled before changing nameservers to prevent connectivity issues.

#### For Google Domains/Squarespace:
1. In domain settings, find **"DNSSEC"** section
2. **Turn OFF** or **Disable DNSSEC**
3. **Note the TTL value** (usually 24 hours)
4. **Wait** for the TTL period before proceeding

### Step 4: Initiate Transfer to Cloudflare

1. In **Cloudflare Dashboard** → **Domain Registration** → **Transfer Domains**
2. **Select domains** available for transfer
3. **Review pricing**: One year will be added to registration
4. **Add payment method** if not already on file
5. **Enter authorization codes** for each domain
6. **Confirm contact information**: Must be accurate for ICANN compliance
7. **Accept terms** and **Confirm Transfer**

### Step 5: Complete Transfer Process

1. **Check email**: Cloudflare will send Form of Authorization (FOA)
2. **Approve transfer**: Most registrars email a confirmation link
   - Click the link to accelerate the process
   - Or wait up to 5 days for automatic processing
3. **Monitor status**: Check Dashboard for transfer progress:
   - "Transfer in progress"
   - "Pending approval" 
   - "Transfer completed"

---

## 3. DNS Configuration for Business Sites

### Basic Business Site DNS Records

#### Essential Records for Most Businesses:

```
Type    Name    Content                 TTL
A       @       YOUR_SERVER_IP          Auto
A       www     YOUR_SERVER_IP          Auto
CNAME   www     yourdomain.com          Auto
MX      @       mail.yourdomain.com     Auto (Priority 10)
TXT     @       "v=spf1 include:..."    Auto
```

### Step-by-Step DNS Configuration

1. **Access DNS Management**:
   - Cloudflare Dashboard → Your Domain → **DNS → Records**

2. **Set Up Root Domain (A Record)**:
   - **Type**: A
   - **Name**: @ (represents your root domain)
   - **IPv4 address**: Your web server IP
   - **Proxy status**: 🟠 Proxied (for Cloudflare protection)

3. **Configure WWW Subdomain**:
   - **Option A - CNAME** (Recommended):
     - **Type**: CNAME
     - **Name**: www
     - **Target**: yourdomain.com
     - **Proxy status**: 🟠 Proxied
   
   - **Option B - A Record**:
     - **Type**: A
     - **Name**: www  
     - **IPv4 address**: Same IP as root domain
     - **Proxy status**: 🟠 Proxied

4. **Email Configuration (if self-hosting)**:
   - **MX Record**:
     - **Type**: MX
     - **Name**: @
     - **Mail server**: mail.yourdomain.com
     - **Priority**: 10
     - **Proxy status**: 🔴 DNS only
   
   - **Mail Server A Record**:
     - **Type**: A
     - **Name**: mail
     - **IPv4 address**: Your mail server IP
     - **Proxy status**: 🔴 DNS only

5. **Google Workspace/Microsoft 365** (if using):
   - Follow provider-specific MX record instructions
   - Usually includes multiple MX records with different priorities

### Common Additional Records

```
Type    Name        Purpose                     Example
CNAME   ftp         File transfer               ftp.yourdomain.com
CNAME   blog        Blog subdomain              blog.yourdomain.com  
A       api         API server                  API_SERVER_IP
TXT     @           Domain verification         "verification-code"
TXT     @           SPF for email               "v=spf1 include:_spf.google.com ~all"
```

### DNS Best Practices

- **Use Cloudflare Proxy** (🟠) for web traffic to get DDoS protection
- **DNS Only** (🔴) for email and FTP servers
- **TTL Settings**: Use "Auto" unless you need specific timing
- **Test Changes**: Use tools like `dig` or online DNS checkers

---

## 4. Cloudflare Pages Deployment from GitHub

### Step 1: Prepare Your GitHub Repository

1. **Create or Use Existing Repo**:
   - Must contain your static site files
   - Can be private or public
   - Ensure build files are gitignored

2. **Typical Project Structure**:
   ```
   my-website/
   ├── src/               # Source files
   ├── public/            # Static assets
   ├── package.json       # Dependencies (if using build tools)
   ├── index.html         # Main page (for simple sites)
   └── README.md
   ```

### Step 2: Connect GitHub to Cloudflare Pages

1. **Access Pages Dashboard**:
   - Cloudflare Dashboard → **Workers & Pages**

2. **Create New Project**:
   - Click **"Create application"**
   - Select **"Pages"**
   - Choose **"Connect to Git"**

3. **GitHub Authorization**:
   - Click **"Connect GitHub"**
   - Authorize Cloudflare Pages app
   - Choose repositories to grant access (or all)

### Step 3: Configure Build Settings

1. **Select Repository**:
   - Choose your website repository from the list
   - Click **"Begin setup"**

2. **Project Configuration**:
   - **Project name**: Will determine your .pages.dev URL
   - **Production branch**: Usually `main` or `master`

3. **Build Settings**:

   #### For Static HTML Sites:
   - **Build command**: Leave empty
   - **Build output directory**: `/` or directory containing index.html

   #### For React/Vue/Angular:
   - **Framework preset**: Select your framework
   - **Build command**: `npm run build` (or framework-specific)
   - **Build output directory**: `dist` or `build`

   #### For Jekyll/Hugo:
   - **Build command**: `hugo` or `bundle exec jekyll build`
   - **Build output directory**: `public` or `_site`

   #### Common Framework Settings:
   ```
   React:          npm run build → build/
   Vue:            npm run build → dist/
   Angular:        ng build → dist/
   Gatsby:         gatsby build → public/
   Next.js:        next build && next export → out/
   Hugo:           hugo → public/
   Jekyll:         jekyll build → _site/
   ```

4. **Environment Variables** (if needed):
   - Add any required build-time variables
   - Example: `NODE_VERSION=18` for specific Node.js version

### Step 4: Deploy Your Site

1. **Save and Deploy**:
   - Click **"Save and Deploy"**
   - Watch build logs in real-time

2. **Build Process**:
   - Cloudflare clones your repository
   - Installs dependencies
   - Runs build command
   - Deploys to global CDN

3. **Access Your Site**:
   - Get unique URL: `your-project.pages.dev`
   - Site is automatically HTTPS-enabled

### Step 5: Set Up Custom Domain

1. **Add Custom Domain**:
   - Go to your Pages project → **Custom domains**
   - Click **"Set up a custom domain"**
   - Enter your domain (e.g., `www.yourdomain.com`)

2. **DNS Configuration**:
   - Add CNAME record in Cloudflare DNS:
     ```
     Type: CNAME
     Name: www (or @)
     Target: your-project.pages.dev
     Proxy: 🟠 Proxied
     ```

3. **SSL Certificate**:
   - Automatically provisioned by Cloudflare
   - Usually takes 5-15 minutes to activate

### Step 6: Automatic Deployments

Every push to your production branch triggers:
1. **Automatic rebuild**
2. **Zero-downtime deployment** 
3. **Global distribution** to 200+ cities

**Preview Deployments**:
- Pull requests create preview URLs
- Test changes before merging
- Each PR gets unique staging URL

---

## 5. Free Tier Limits and Paid Plan Considerations

### Cloudflare Free Plan Limits

| Resource | Free Plan Limit | Notes |
|----------|-----------------|-------|
| **Pages Builds** | 500 builds/month | Each git push = 1 build |
| **Build Time** | 20 minutes max | Per build timeout |
| **File Count** | 20,000 files | Per site |
| **File Size** | 25 MiB max | Per individual file |
| **Custom Domains** | 100 per project | More than enough for most |
| **Sites/Projects** | 100 per account | Soft limit, can request increase |
| **Bandwidth** | Unlimited | No limits on traffic |
| **Preview Deployments** | Unlimited | All branches get previews |

### When You Need Paid Plans

#### Upgrade to Pro ($20/month) when you need:
- **More builds**: 5,000 builds/month
- **Advanced analytics**: Detailed traffic insights
- **More custom domains**: 250 per project  
- **Priority support**: Faster response times

#### Upgrade to Business ($200/month) when you need:
- **Even more builds**: 20,000 builds/month
- **Advanced security**: WAF, DDoS protection
- **More file limits**: 100,000 files per site
- **Teams collaboration**: User management features

#### Enterprise Plan when you need:
- **Unlimited builds**
- **SLA guarantees**
- **Dedicated support**
- **Advanced compliance** features

### Cost Calculation Examples

**Typical Small Business**:
- 1 main site + 2 staging environments
- ~50 builds/month (updates 1-2x per week)
- **Cost**: Free plan sufficient

**Active Development Team**:
- 3 developers × 2 pushes/day × 22 workdays = 132 builds/month
- **Cost**: Free plan sufficient

**High-frequency Updates**:
- Daily deployments + frequent hotfixes = 600+ builds/month
- **Cost**: Need Pro plan ($20/month)

### Free Tier Optimization Tips

1. **Combine Related Changes**: Bundle multiple commits before pushing
2. **Use Branches Wisely**: Only push to main when ready to deploy
3. **Optimize Build Times**: Reduce dependencies, use caching
4. **Monitor Usage**: Dashboard shows build count and limits

---

## 6. SSL/Security Setup

### Automatic SSL Features (Free)

Cloudflare provides **Universal SSL** automatically:
- ✅ **Free SSL certificates** for all domains
- ✅ **Automatic renewal** (Let's Encrypt)
- ✅ **Global distribution** via CDN
- ✅ **Modern TLS** (1.2+)

### SSL Configuration Steps

1. **Verify SSL Status**:
   - Dashboard → **SSL/TLS** → **Overview**
   - Should show "Active Certificate"

2. **Choose SSL Mode**:
   ```
   Off: ❌ No encryption (never use)
   Flexible: ⚠️ Cloudflare to visitor only
   Full: ✅ End-to-end, allows self-signed
   Full (Strict): 🔒 End-to-end, valid certificates only
   ```

   **Recommendation**: Use **"Full (Strict)"** for maximum security

3. **Enable HSTS** (HTTP Strict Transport Security):
   - **SSL/TLS** → **Edge Certificates**
   - Enable **"HTTP Strict Transport Security (HSTS)"**
   - **Max Age**: 12 months
   - **Include subdomains**: Yes
   - **No-Sniff header**: Yes

### Security Best Practices

#### 1. Enable Security Features
- **Always Use HTTPS**: Redirect all HTTP to HTTPS
- **Minimum TLS Version**: Set to TLS 1.2 or higher
- **HSTS**: Force HTTPS for repeat visitors

#### 2. Configure Page Rules for Security
```
Pattern: http://*yourdomain.com/*
Settings: Always Use HTTPS
```

#### 3. Additional Security Options

**Browser Integrity Check**: ✅ Enable
- Blocks common threats and suspicious browsers

**Challenge Passage**: 
- **Under Attack Mode**: Use during DDoS attacks
- **High**: For sensitive areas
- **Medium**: Standard protection
- **Low**: Light protection
- **Essentially Off**: Minimal protection

**Bot Fight Mode**: ✅ Enable
- Free DDoS protection against automated threats

#### 4. DNS Security Features

**DNSSEC**: ✅ Enable after transfer completes
- **SSL/TLS** → **Edge Certificates** → **DNSSEC**
- Protects against DNS spoofing attacks

### Advanced Security (Paid Features)

#### Web Application Firewall (WAF)
- **Pro Plan+**: Custom firewall rules
- **Block countries**: Geo-based blocking
- **Rate limiting**: Prevent abuse
- **Custom rules**: Advanced threat protection

#### DDoS Protection
- **Free**: Basic L3/L4 protection
- **Pro+**: Advanced analytics and reporting
- **Business+**: L7 DDoS protection

### Security Monitoring

1. **Security Analytics**:
   - Dashboard → **Security** → **Analytics**
   - Monitor threats and blocked requests

2. **Security Events**:
   - Real-time threat feed
   - Country-based attack patterns
   - Bot vs. human traffic ratios

---

## 7. Troubleshooting and Best Practices

### Common Issues and Solutions

#### Domain Transfer Problems

**"Transfer Rejected"**:
- ✅ Check domain is unlocked at current registrar
- ✅ Verify authorization code is correct and recent
- ✅ Ensure WHOIS contact info hasn't changed recently
- ✅ Domain must be >60 days old

**"DNS Errors After Transfer"**:
- ✅ Verify all DNS records were imported correctly
- ✅ Check TTL values haven't caused caching issues
- ✅ Ensure MX records are set to "DNS Only" (🔴)

#### Pages Deployment Issues

**Build Failures**:
```bash
# Common fixes:
- Check Node.js version in environment variables
- Verify package.json has correct dependencies
- Ensure build command is framework-appropriate
- Check for case-sensitive file path issues
```

**"Site Not Loading"**:
- ✅ Verify build output directory is correct
- ✅ Check index.html exists in output folder
- ✅ Ensure custom domain DNS is properly configured

#### SSL/Security Issues

**"Mixed Content Warnings"**:
- ✅ Update all HTTP links to HTTPS
- ✅ Use relative URLs where possible
- ✅ Enable "Automatic HTTPS Rewrites"

**"Certificate Not Valid"**:
- ✅ Wait 15 minutes for initial certificate provisioning
- ✅ Verify domain is pointed to Cloudflare nameservers
- ✅ Check custom domain is properly configured

### Performance Optimization

#### Cloudflare Settings for Speed

1. **Caching Configuration**:
   - **Caching Level**: Standard
   - **Browser Cache TTL**: 4 hours to 1 month
   - **Auto Minify**: Enable CSS, JS, HTML

2. **Speed Optimizations**:
   - **Brotli Compression**: ✅ Enable
   - **HTTP/2**: ✅ Automatic
   - **HTTP/3 (QUIC)**: ✅ Enable for fastest connections

#### Content Optimization

1. **Image Optimization**:
   - Use WebP format when possible
   - Enable **Polish** (Pro plan) for automatic optimization
   - **Mirage** (Pro plan) for mobile optimization

2. **JavaScript/CSS**:
   - Enable **Rocket Loader** for JS optimization
   - **Auto Minify** for smaller file sizes

### Best Practices Summary

#### Domain Management
- ✅ Keep domain registration separate from hosting decisions
- ✅ Enable auto-renewal to prevent accidental expiration
- ✅ Maintain current contact information for ICANN compliance
- ✅ Use strong passwords and 2FA on registrar accounts

#### DNS Management  
- ✅ Use Cloudflare proxy (🟠) for web traffic
- ✅ Keep email services on DNS only (🔴)
- ✅ Document all DNS changes before making them
- ✅ Test changes on staging domains first when possible

#### Pages Deployment
- ✅ Use meaningful branch names for preview deployments
- ✅ Keep build times under 10 minutes when possible
- ✅ Monitor build count to avoid hitting free tier limits
- ✅ Use environment variables for configuration, not hardcoded values

#### Security
- ✅ Always use "Full (Strict)" SSL mode
- ✅ Enable HSTS for enhanced security
- ✅ Regularly review Security Analytics for threats
- ✅ Keep Cloudflare security settings up to date

### Getting Help

- **Cloudflare Community**: [community.cloudflare.com](https://community.cloudflare.com)
- **Documentation**: [developers.cloudflare.com](https://developers.cloudflare.com)
- **Support**: Available based on plan level
- **Status Page**: [cloudflarestatus.com](https://cloudflarestatus.com)

---

## Summary Checklist

Use this checklist to ensure you've completed all setup steps:

### Pre-Transfer
- [ ] Created Cloudflare account
- [ ] Domain is >60 days old
- [ ] Documented current DNS settings
- [ ] Notified financial institution

### Domain Transfer
- [ ] Domain unlocked at current registrar
- [ ] Authorization code obtained
- [ ] DNSSEC disabled (waited for TTL)
- [ ] Domain added to Cloudflare
- [ ] Nameservers updated
- [ ] Transfer initiated and completed

### DNS Configuration
- [ ] Root domain A record configured
- [ ] WWW subdomain configured
- [ ] Email MX records set up
- [ ] All necessary subdomains configured
- [ ] Proxy settings optimized

### Pages Deployment
- [ ] GitHub repository connected
- [ ] Build settings configured correctly
- [ ] Custom domain added and verified
- [ ] SSL certificate active
- [ ] Automatic deployments working

### Security & Performance
- [ ] SSL mode set to "Full (Strict)"
- [ ] HSTS enabled
- [ ] DNSSEC activated
- [ ] Security features optimized
- [ ] Performance settings configured

---

*Guide created: January 2026*
*Last updated: January 2026*

This guide provides a comprehensive foundation for using Cloudflare's domain and hosting services. As Cloudflare continues to evolve, always refer to their official documentation for the most current procedures and features.