# ALETHEIA: Complete Capability Research
## What's Possible Today vs Tomorrow

*Research compiled: February 2026*

---

# EXECUTIVE SUMMARY

This document maps every capability an autonomous AI agent system could possess, categorized by:
- **Production-Ready** - Available today with APIs
- **Emerging** - Available but requires integration work
- **Theoretical** - Coming 2026-2027+
- **Native** - Can be done without external tools
- **Requires Tooling** - Needs specific APIs/hardware

---

# 1. VOICE & PHONE

## Production-Ready TODAY

| Platform | Type | Latency | Pricing | API |
|----------|------|---------|---------|-----|
| **Bland AI** | Outbound calls at scale | Sub-second | $0.09/min | Yes |
| **Retell AI** | All-in-one voice agent | Sub-second | $0.07-0.15/min | Yes |
| **Vapi** | Developer-focused SDK | <600ms | $0.30-0.33/min | Yes |
| **ElevenLabs** | TTS leader | 75ms (Flash) | $0.05-0.30/min | Yes |
| **OpenAI Realtime** | Native speech-to-speech | Real-time | $0.16-1.63/min | Yes |
| **Deepgram** | STT leader | <300ms | $0.0077/min | Yes |

### What Works
- **Outbound calls**: Appointment scheduling, reminders, follow-ups
- **Inbound handling**: Customer service, routing, FAQ
- **Voice cloning**: Match user's voice in seconds
- **Multi-language**: 30+ languages supported

### What's Challenging
- Multi-party calls (cross-talk issues)
- Very long calls (context limits)
- Complex emotional situations

### Self-Hosted Option
- **Pipecat** (open source): Full STT-LLM-TTS pipeline
- **LiveKit Agents**: Open source framework
- **Requires**: 16GB+ RAM, GPU recommended

### Compliance Requirements
- TCPA: AI voices explicitly covered (Feb 2024 FCC ruling)
- Two-party consent states require disclosure
- Must disclose AI nature at call start

**Sources:**
- [Bland AI](https://www.bland.ai/)
- [Retell AI](https://www.retellai.com/)
- [Vapi](https://vapi.ai/)
- [ElevenLabs](https://elevenlabs.io/)
- [OpenAI Realtime API](https://platform.openai.com/docs/guides/realtime)
- [Pipecat GitHub](https://github.com/pipecat-ai/pipecat)

---

# 2. BROWSER & COMPUTER USE

## Production-Ready TODAY

| Platform | Benchmark | Pricing | Access |
|----------|-----------|---------|--------|
| **Claude Computer Use** | 72.7% OSWorld (Opus 4.6) | API pricing | Beta header required |
| **Claude Cowork** | Desktop automation | Pro/Max subscription | macOS only |
| **Browser Use** | 89% WebVoyager | Free/open source | Python library |
| **Browserbase** | Enterprise browser infra | Custom | API |

### What Works Reliably
- Form filling (90%+ accuracy on standard forms)
- Multi-step web workflows (80% reduction in processing time)
- Screenshot understanding
- Login/authentication with cookie persistence

### What's Challenging
- CAPTCHA (fundamental limitation - behavioral analysis detects AI)
- Complex multi-site workflows with unexpected variations
- Time-critical interactions (latency issues)

### Self-Hosted Options
- **Browser Use**: Open source, 77.9k GitHub stars
- **Skyvern**: LLM + computer vision, Docker-based
- **BrowserOS**: Open source browser with built-in AI

### Claude Computer Use Specifics
```
Supported: Screenshot capture, mouse control, keyboard input, zoom
Limitations: Latency (~3-5s per action), coordinate hallucination possible
Pricing: Opus 4.5 = $5/$25 per MTok, Sonnet 4.5 = $3/$15 per MTok
```

**Sources:**
- [Claude Computer Use](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool)
- [Browser Use](https://github.com/browser-use/browser-use)
- [Browserbase](https://www.browserbase.com/)
- [Skyvern](https://github.com/Skyvern-AI/skyvern)

---

# 3. PAYMENTS & FINANCIAL

## Production-Ready TODAY

| Platform | Capability | API | Status |
|----------|------------|-----|--------|
| **Stripe Agent Toolkit** | Create products, payment links, virtual cards | Free SDK | Live |
| **Plaid MCP** | Bank account access, diagnostics | MCP server | Live |
| **QuickBooks AI** | Invoice creation, reconciliation | REST API | Live |

### What Agents CAN Do Now
- Read financial data (balances, transactions)
- Create invoices and payment links
- Categorize expenses automatically
- Generate single-use virtual cards (Stripe Issuing)
- Monitor and alert on spending patterns

### Emerging (2026)

| Protocol | Status | General Availability |
|----------|--------|---------------------|
| **Visa Trusted Agent** | Developer preview | Early 2026 |
| **Google AP2** | Announced, docs available | 2026 |
| **PayPal Agent Ready** | Announced | Early 2026 |
| **Mastercard Agent Pay** | Live (US cardholders) | Holiday 2025 (US), 2026 (global) |
| **ACP (OpenAI/Stripe)** | Beta | Now |

### Security Requirements
- PCI DSS compliance required
- Tokenization mandatory (agents never see raw card numbers)
- Audit trails for all agent actions
- Human approval for high-value transactions

### Self-Hosted Options
- **Hyperswitch**: Open source payment orchestrator
- **Lago**: Open source billing platform
- Limitation: Still need processor relationship (Stripe, etc.)

**Sources:**
- [Stripe Agent Toolkit](https://docs.stripe.com/agents)
- [Plaid MCP](https://plaid.com/blog/openai-plaid-mcp/)
- [Visa Trusted Agent Protocol](https://developer.visa.com/capabilities/trusted-agent-protocol)
- [Google AP2](https://cloud.google.com/blog/products/ai-machine-learning/announcing-agents-to-payments-ap2-protocol)
- [Hyperswitch](https://hyperswitch.io/)

---

# 4. SMART HOME & IoT

## Production-Ready TODAY

| Platform | Integration | Voice | Self-Hosted |
|----------|-------------|-------|-------------|
| **Home Assistant** | 1000+ devices | Wyoming/Assist | Yes |
| **Matter** | Cross-platform standard | Via HA | N/A |
| **Zigbee/Z-Wave** | Most reliable protocols | Via HA | Yes |

### Home Assistant AI Capabilities
- Chat with any LLM (cloud or local via Ollama)
- Hybrid approach: Simple commands → Assist, Complex → LLM
- Automation suggestions via AI
- MCP servers available for Claude integration

### What Works Reliably
- Lighting control (scenes, dimming, color)
- Climate management
- Lock/security control
- Presence detection (mmWave sensors 95%+ accuracy)

### Voice Control Options

| Option | Latency | Privacy | Hardware |
|--------|---------|---------|----------|
| Cloud (Alexa/Google) | <1s | Low | Any |
| Local (Whisper + Piper) | 1-8s | High | Pi 4+ |
| Hybrid | <1s | Medium | Intel N100+ |

### Self-Hosted Stack
```
Hub: Raspberry Pi 5 or Intel N100
Coordinator: Zigbee2MQTT + Sonoff ZBDongle-P
Voice: Home Assistant Voice PE ($59)
LLM: Ollama on separate mini PC
Total cost (3 rooms): ~$340
```

### MCP Integration
- [HA_MCP](https://github.com/mtebusi/ha_mcp): Natural language control of entire smart home
- OAuth2 + SSL, zero-config

**Sources:**
- [Home Assistant AI](https://www.home-assistant.io/blog/2025/09/11/ai-in-home-assistant)
- [Wyoming Protocol](https://www.home-assistant.io/integrations/wyoming/)
- [Matter Standard 2026](https://matter-smarthome.de/en/development/the-matter-standard-in-2026-a-status-review/)
- [HA_MCP GitHub](https://github.com/mtebusi/ha_mcp)

---

# 5. HEALTH & BIOMETRICS

## Production-Ready APIs

| Platform | Data Available | API | Granularity |
|----------|---------------|-----|-------------|
| **Oura** | Sleep, HRV, readiness, activity | OAuth 2.0 REST | Daily + 5-min nighttime |
| **WHOOP** | Recovery, strain, sleep, workouts | OAuth 2.0 + Webhooks | Real-time |
| **Garmin** | Sleep, stress, activity, training | OAuth 2.0 REST | Webhooks available |
| **Dexcom** | Glucose readings | OAuth 2.0 REST | Every 5 minutes |
| **Apple HealthKit** | 100+ metrics | iOS app only | N/A (no remote API) |

### Apple HealthKit Limitation
- **No server-side API exists**
- Must have native iOS app installed
- Workarounds: Export apps, FHIR converters

### Unified Aggregators

| Platform | Devices | Self-Hosted |
|----------|---------|-------------|
| **Terra API** | 150+ sources | No (SaaS) |
| **Open Wearables** | 200+ sources | Yes (MIT license) |
| **Vital** | 500+ wearables + labs | No |

### AI Analysis Capabilities
- Trend detection across metrics
- Anomaly alerting (deviation from baseline)
- Personalized recovery recommendations
- Cross-metric correlation

### Privacy Considerations
- HIPAA applies when data enters clinical settings
- Consumer wearables generally not covered
- Self-hosted options: Open Wearables, Fasten Health

**Sources:**
- [Oura API v2](https://cloud.ouraring.com/v2/docs)
- [WHOOP Developer Platform](https://developer.whoop.com/)
- [Garmin Connect Developer](https://developer.garmin.com/gc-developer-program/)
- [Dexcom Developer Portal](https://developer.dexcom.com/)
- [Open Wearables](https://github.com/the-momentum/open-wearables)
- [Terra API](https://tryterra.co/)

---

# 6. DOCUMENTS & LEGAL

## Production-Ready TODAY

| Tool | Specialty | Accuracy | API |
|------|-----------|----------|-----|
| **Mistral OCR 3** | All document types | 99%+ | Yes, $2/1000 pages |
| **LegalOn** | Contract review | 92/100 | Enterprise |
| **Harvey** | Am Law firms | 88/100 | Enterprise |
| **DocuSign** | E-signatures | N/A | REST API |
| **Docling** | Open source processing | Production-grade | Python library |

### What Works Reliably
- Document classification: 90-98% accuracy
- Data extraction from clean PDFs: 95-98%
- Handwriting recognition: 95%+ with LLM-powered solutions
- Multi-page document handling

### Contract Review Capabilities
- Clause extraction: 94.2% F1 score (Sirion benchmark)
- Risk identification and flagging
- Compliance checking against regulations
- Auto-redlining: 90%+ accuracy (Dioptra)

### Open Source Stack
```
OCR: Mistral OCR 3 or PaddleOCR 3.0
Processing: Docling (Linux Foundation)
E-signature: DocuSeal (self-hosted)
Integration: LlamaIndex for RAG
```

### Self-Hosted Cost Comparison
- Cloud OCR: ~$1.50/1,000 pages
- Self-hosted (A100): ~$0.09/1,000 pages (16x cheaper)

**Sources:**
- [Mistral OCR 3](https://mistral.ai/news/mistral-ocr-3)
- [Docling](https://github.com/docling-project/docling)
- [LegalOn](https://www.legalontech.com/)
- [DocuSeal](https://github.com/docusealco/docuseal)
- [LlamaIndex Document AI](https://www.llamaindex.ai/blog/document-ai-the-next-evolution-of-intelligent-document-processing)

---

# 7. CODING & DEVELOPMENT

## Production-Ready TODAY

| Tool | SWE-bench | Cost | Self-Hosted |
|------|-----------|------|-------------|
| **Claude Code** | 80.9% | $100-200/mo API | No |
| **Cursor** | ~77% | $20-200/mo | No |
| **Windsurf** | Leader in Gartner MQ | $15-60/mo | No |
| **Devin** | 13.86% E2E | $20-500+/mo | VPC option |
| **Aider** | SOTA on main bench | API costs only | Yes |

### What Works Reliably
- Code generation: 70-82% accuracy on standard tasks
- Multi-file refactoring: Strong with Cursor, Claude Code
- Test generation: 2-3x improvement in testing efforts
- Bug detection: 42-48% improvement with AI

### What's Challenging
- Large codebase understanding (100K+ files)
- Architectural decisions
- Security: 45% of AI-generated code contains security flaws

### Self-Hosted Alternatives
- **Tabby**: Self-hosted Copilot alternative
- **Continue**: Model-agnostic, local models supported
- **Aider**: CLI tool, works with Ollama

### MCP Servers for Coding
- **GitHub MCP**: Read repos, manage PRs, automate workflows
- **Git MCP**: Local repo understanding
- **GitLab MCP**: CI/CD pipeline integration

**Sources:**
- [Claude Code](https://platform.claude.com/docs/en/agents-and-tools/claude-code)
- [Cursor](https://cursor.com/)
- [Windsurf](https://windsurf.com/)
- [Aider](https://aider.chat/)
- [Tabby](https://github.com/TabbyML/tabby)
- [GitHub MCP Server](https://github.com/github/github-mcp-server)

---

# 8. RESEARCH & LEARNING

## Production-Ready TODAY

| Platform | Papers | Specialty | API |
|----------|--------|-----------|-----|
| **Elicit** | 125M+ | Systematic reviews | Enterprise |
| **Consensus** | 250M+ | Medical/Deep Search | By application |
| **Perplexity** | Web + Academic | Rapid synthesis | Yes |
| **Semantic Scholar** | 225M+ | Citation graphs | Free (1 RPS) |

### What Works Reliably
- Paper discovery and search
- Citation extraction and analysis
- Summary generation with sources
- Evidence synthesis: 47.8x faster than human review

### Accuracy Notes
- Abstract screening: 73.8-86.0%
- PICO extraction: >85% median accuracy
- Limitation: AI should complement, not replace, human reviewers

### Self-Hosted Options
- **Paperlib**: Open source paper management with LLM
- **OpenAlex API**: Free, 100K credits/day, CC0 data
- **Local RAG**: FAISS/Qdrant + local LLM

**Sources:**
- [Elicit](https://elicit.com/)
- [Consensus](https://consensus.app/)
- [Perplexity](https://www.perplexity.ai/)
- [Semantic Scholar API](https://www.semanticscholar.org/product/api)
- [OpenAlex](https://openalex.org/)
- [Paperlib](https://github.com/Future-Scholars/paperlib)

---

# 9. CREATIVE & MEDIA

## Image Generation

| Platform | Strength | API | Pricing |
|----------|----------|-----|---------|
| **DALL-E 3** | Prompt accuracy | OpenAI API | $0.04-0.12/image |
| **Flux Pro/2** | Photorealism | FAL, Replicate | $0.03-0.06/megapixel |
| **Midjourney** | Artistic quality | **No official API** | $10-60/mo subscription |
| **Stable Diffusion** | Customization | Self-hosted | Free |

## Video Generation

| Platform | Max Length | Resolution | API | Pricing |
|----------|------------|------------|-----|---------|
| **Sora 2** | 60s | 4K | Yes | $0.10-0.50/second |
| **Runway Gen-4** | 10s+ | 4K | Yes | ~$0.01/credit |
| **Kling 2.6** | 2 min | 1080p | Via FAL | ~$0.07-0.14/second |
| **Veo 3.1** | 60s | 4K | Vertex AI | $0.15-0.75/second |

## Audio/Music

| Platform | Capability | API | Commercial Use |
|----------|------------|-----|----------------|
| **ElevenLabs** | Voice cloning, TTS | Yes | Yes |
| **Suno v5** | Complete songs | **No** | Paid tiers |
| **Udio** | High fidelity audio | **No** | Paid tiers |

### Self-Hosted Options
- **ComfyUI + Stable Diffusion/Flux**: Most flexible
- **WAN 2.x**: Video generation on consumer GPU (8GB+)
- **Mochi 1**: Video on professional GPU (24GB+)

### Best for Agent Integration
1. **Images**: Flux via Together AI (free tier) or DALL-E 3
2. **Video**: Runway Gen-4 (best balance)
3. **Voice**: ElevenLabs (comprehensive API)
4. **Music**: No reliable APIs - use asset libraries

**Sources:**
- [OpenAI DALL-E 3](https://platform.openai.com/docs/models/dall-e-3)
- [Runway API](https://docs.dev.runwayml.com/)
- [Sora API](https://platform.openai.com/docs/guides/video-generation)
- [ElevenLabs](https://elevenlabs.io/developers)
- [ComfyUI](https://github.com/comfyanonymous/ComfyUI)

---

# 10. EMAIL & CALENDAR

## Production-Ready APIs

| Platform | Coverage | Auth | Features |
|----------|----------|------|----------|
| **Gmail API** | Google only | OAuth 2.0 | Full CRUD, Pub/Sub |
| **Google Calendar** | Google only | OAuth 2.0 | Events, free/busy |
| **Microsoft Graph** | M365 | OAuth 2.0 | Unified mail/calendar |
| **Nylas** | Multi-provider | OAuth 2.0 | $1-1.50/account |

### AI Email Tools

| Tool | Specialty | Pricing |
|------|-----------|---------|
| **Shortwave** | Gmail AI, Ghostwriter | Free tier available |
| **Superhuman** | Speed, AI features | $30-40/mo |
| **Spark +AI** | Cross-platform, affordable | $59.99/year |
| **Lindy** | No-code email agents | Free (400 tasks) |

### What Works Reliably
- Email classification: 90%+ with behavioral learning
- Auto-drafting: 90% reduction in response time
- Calendar scheduling: 60-80% reduction in coordination time

### Self-Hosted Options
- **Inbox Zero**: Open source AI email assistant
- **Zero (Mail-0)**: Open source AI email client
- **n8n + Ollama**: Local processing, no external calls
- **Cal.com**: Open source scheduling

**Sources:**
- [Gmail API](https://developers.google.com/workspace/gmail/api)
- [Google Calendar API](https://developers.google.com/workspace/calendar)
- [Microsoft Graph](https://learn.microsoft.com/en-us/graph/)
- [Nylas](https://www.nylas.com/)
- [Shortwave](https://www.shortwave.com/)
- [Inbox Zero](https://github.com/elie222/inbox-zero)
- [Cal.com](https://github.com/calcom/cal.com)

---

# 11. VEHICLE INTEGRATION

## Production-Ready APIs

| Platform | Brands | Data | Commands | API |
|----------|--------|------|----------|-----|
| **Tesla Fleet** | Tesla | Location, battery, diagnostics | Lock, climate, charge | OAuth 2.0 |
| **Smartcar** | 40+ brands | Location, fuel, odometer | Lock, charge | OAuth 2.0 |
| **High Mobility** | 500+ models | 300+ data points | Varies | REST |

### OBD2 Diagnostic Integration

| Device | Type | Price | Best For |
|--------|------|-------|----------|
| **OBDLink CX** | BLE | $80-100 | Premium quality |
| **Vgate iCar Pro** | BLE 4.0 | $30-50 | Budget option |
| **Freematics ONE+** | Cellular | $100-150 | DIY telematics |

### AI Diagnostic Capabilities
- Pattern recognition across DTCs
- Predictive maintenance: 97.5% R2 accuracy
- Cost reduction: Up to 30%

### Self-Hosted Options
- **TeslaMate**: Tesla-specific, Grafana dashboards
- **Traccar**: 200+ GPS protocols, multi-vehicle
- **OVMS**: Open Vehicle Monitoring System (hardware module)

### Home Assistant Integration
- Tesla Fleet: Built-in since 2024.6
- Ford/Hyundai/Kia: HACS custom integrations
- OBD2: Via Bluetooth → local network

**Sources:**
- [Tesla Fleet API](https://developer.tesla.com/docs/fleet-api/getting-started/what-is-fleet-api)
- [Smartcar](https://smartcar.com/)
- [TeslaMate](https://github.com/teslamate-org/teslamate)
- [OVMS](https://www.openvehicles.com/)
- [Traccar](https://www.traccar.org/)

---

# 12. ROBOTICS & PHYSICAL WORLD

## Current State (February 2026)

### Humanoid Robots

| Robot | Status | Price | Availability |
|-------|--------|-------|--------------|
| **1X NEO** | Pre-orders open | $20,000 or $499/mo | Q3-Q4 2026 |
| **Tesla Optimus** | Factory use only | $25-30K target | Late 2026 (maybe) |
| **Figure 03** | Industrial only | Not disclosed | No consumer sales |
| **Boston Dynamics Atlas** | Enterprise only | $1M+ | Industrial deployments |

### Actually Available Now

| Category | Top Products | Integration |
|----------|--------------|-------------|
| **Robot vacuums** | Roborock S8 MaxV, Dreame X50 | Home Assistant, Matter |
| **Robot mowers** | Husqvarna, EEVE Willow | Limited API |
| **Delivery robots** | Starship (commercial) | B2B only |

### Robot Vacuum Integration

| Method | Features |
|--------|----------|
| **Home Assistant native** | Roomba, Roborock, Dreame, ECOVACS |
| **Matter** | Roborock, Dreame X40/X50 (via firmware) |
| **Valetudo** | Privacy-first local control (open source) |

### Realistic Timeline
- **2026**: First consumer humanoids ship ($20K+)
- **2027-2028**: Broader availability, prices dropping
- **2028-2030**: Practical home humanoids under $10K

### Best Current Investment
- Premium robot vacuum with Matter support
- Home Assistant integration
- Valetudo for privacy if supported

**Sources:**
- [1X NEO](https://www.1x.tech/discover/neo-home-robot)
- [Tesla Optimus](https://www.tesla.com/we-robot)
- [Figure AI](https://www.figure.ai/)
- [Boston Dynamics Atlas](https://bostondynamics.com/atlas/)
- [Valetudo](https://valetudo.cloud/)
- [Home Assistant Roborock](https://www.home-assistant.io/integrations/roborock/)

---

# CAPABILITY MATRIX

## What's Native vs Requires Tooling

| Capability | Native | Requires Tooling |
|------------|--------|------------------|
| Text generation | Yes | - |
| Code generation | Yes | - |
| Research/analysis | Yes | Web search tools |
| Phone calls | No | Bland/Vapi/Retell |
| Send email | No | Gmail/Graph API |
| Make payments | No | Stripe/Plaid |
| Control smart home | No | Home Assistant |
| Browse web | No | Computer Use/Browser Use |
| Generate images | No | DALL-E/Flux API |
| Generate video | No | Runway/Sora API |
| Access health data | No | Oura/WHOOP API |
| Vehicle control | No | Tesla/Smartcar API |
| Document processing | No | Mistral OCR/Docling |
| Schedule meetings | No | Calendar APIs |

## Production Readiness by Domain

| Domain | Production Ready | Emerging | Theoretical |
|--------|-----------------|----------|-------------|
| Voice/Phone | 90% | 10% | - |
| Browser/Computer | 70% | 25% | 5% |
| Payments | 40% | 50% | 10% |
| Smart Home | 85% | 15% | - |
| Health/Wearables | 75% | 20% | 5% |
| Documents/Legal | 80% | 15% | 5% |
| Coding | 80% | 15% | 5% |
| Research | 85% | 10% | 5% |
| Creative/Media | 75% | 20% | 5% |
| Email/Calendar | 90% | 10% | - |
| Vehicle | 60% | 30% | 10% |
| Robotics | 20% | 30% | 50% |

---

# RECOMMENDED INTEGRATION PRIORITY

## Phase 1: Foundation (Now)
1. **Graphiti** - Temporal knowledge graph (replaces custom scripts)
2. **Langfuse** - Observability for all nous sessions
3. **Reflexion pattern** - Structured self-improvement

## Phase 2: Communication (30 days)
1. **Voice mode** - ElevenLabs + OpenAI Realtime
2. **Phone calls** - Bland AI or Vapi
3. **Enhanced email** - AI prioritization + auto-drafting

## Phase 3: Browser & Desktop (60 days)
1. **Claude Computer Use** - Complex multi-step tasks
2. **Browser Use** - Web automation
3. **Document processing** - Docling + Mistral OCR

## Phase 4: Home & Vehicle (90 days)
1. **Home Assistant** - Full smart home integration
2. **OBD2 monitoring** - Vehicle health via Bluetooth
3. **Biometric ingestion** - Oura/Watch APIs

## Phase 5: Financial & Professional (120 days)
1. **Payment capability** - Start with bill pay
2. **Work tool integrations** - Jira, Linear, etc.
3. **Social media automation** - For business use

## Phase 6: Physical World (2026+)
1. Robot vacuum integration via Valetudo
2. Monitor humanoid market for home availability
3. Prepare infrastructure for physical agent control

---

# OPEN SOURCE STACK SUMMARY

| Layer | Recommended Tool | Purpose |
|-------|------------------|---------|
| Graph Memory | Graphiti | Temporal knowledge graph |
| Observability | Langfuse | Session tracing |
| Voice | Pipecat | STT-LLM-TTS pipeline |
| Browser | Browser Use | Web automation |
| Home | Home Assistant | Smart home control |
| Documents | Docling | PDF/document processing |
| Email | Inbox Zero | AI email assistant |
| Calendar | Cal.com | Scheduling |
| Coding | Aider | AI pair programming |
| Workflow | Temporal | Durable orchestration |
| Vacuum | Valetudo | Local robot control |
| Vehicle | TeslaMate/Traccar | Telematics |

---

# APPROVAL ARCHITECTURE

| Action Type | Default | Examples |
|-------------|---------|----------|
| Read-only | Auto | Check calendar, read email |
| Low-stakes write | Notify | Add calendar event, draft email |
| Medium-stakes | Confirm | Send email, purchase <$50 |
| High-stakes | Block until approved | Payments >$50, delete data |
| Irreversible | Double confirm | Contract signatures |

---

# THE VISION

Aletheia becomes a **distributed cognitive system** that can:

- **See** everything (email, calendar, health, home, vehicle, web)
- **Think** across domains (graph memory, cross-nous deliberation)
- **Act** in the world (calls, payments, automation, physical tasks)
- **Learn** continuously (reflexion, knowledge accumulation)
- **Coordinate** seamlessly (shared awareness, delegation)

Not an assistant. A **partner with agency**.

---

*Document generated by Claude Code for Aletheia development*
*Research sources cited throughout*
