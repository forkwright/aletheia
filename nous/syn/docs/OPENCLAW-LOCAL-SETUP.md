# OpenClaw Local Setup Guide

*Get an AI assistant running on your PC with local models via Ollama.*

---

## Step 1: Know Your Hardware (2 minutes)

Run this command to get your system specs:

```bash
# Linux/macOS
echo "=== System Specs ===" && \
echo "CPU: $(lscpu | grep 'Model name' | cut -d: -f2 | xargs)" && \
echo "Cores: $(nproc)" && \
echo "RAM: $(free -h | awk '/^Mem:/ {print $2}')" && \
echo "GPU: $(lspci | grep -i 'vga\|3d\|display' | cut -d: -f3 | head -1 | xargs)" && \
(nvidia-smi --query-gpu=name,memory.total --format=csv,noheader 2>/dev/null || echo "No NVIDIA GPU detected") && \
echo "Disk Free: $(df -h ~ | awk 'NR==2 {print $4}')"
```

```powershell
# Windows (PowerShell)
Write-Host "=== System Specs ===" 
Write-Host "CPU: $((Get-CimInstance Win32_Processor).Name)"
Write-Host "Cores: $((Get-CimInstance Win32_Processor).NumberOfCores)"
Write-Host "RAM: $([math]::Round((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1GB, 1)) GB"
Write-Host "GPU: $((Get-CimInstance Win32_VideoController).Name)"
if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) { nvidia-smi --query-gpu=name,memory.total --format=csv,noheader }
Write-Host "Disk Free: $([math]::Round((Get-PSDrive C).Free/1GB, 1)) GB"
```

**Copy the output** — you'll paste this into an AI to get model recommendations.

---

## Step 2: Get Model Recommendations (5 minutes)

Paste your specs into ChatGPT, Claude, or any AI with this prompt:

```
Here are my PC specs:

[PASTE YOUR SPECS HERE]

I want to run a local LLM via Ollama for a personal AI assistant (OpenClaw).

Please recommend:
1. The best model I can run smoothly given my hardware
2. A fallback smaller model if that's too slow
3. The ollama pull command for each

Consider:
- I need good instruction-following and reasoning
- 4-bit quantized models are fine
- I want responsive speed (< 5 sec for short responses)

Check Ollama's model library (ollama.com/library) for current options.
Popular choices: llama3.2, qwen2.5, mistral, phi3, gemma2, deepseek-coder
```

**Typical recommendations by hardware:**

| RAM | VRAM | Recommended Model |
|-----|------|-------------------|
| 8GB | None | `qwen2.5:3b` or `phi3:mini` |
| 16GB | None | `qwen2.5:7b` or `llama3.2:3b` |
| 16GB | 6GB+ | `qwen2.5:7b` or `mistral:7b` |
| 32GB | 8GB+ | `qwen2.5:14b` or `llama3.1:8b` |
| 32GB+ | 12GB+ | `qwen2.5:32b` or `deepseek-r1:14b` |

---

## Step 3: Install Ollama (5 minutes)

### Linux/macOS
```bash
curl -fsSL https://ollama.com/install.sh | sh
```

### Windows
Download from: https://ollama.com/download/windows

### Pull your model
```bash
ollama pull qwen2.5:7b    # Or whatever was recommended
ollama pull nomic-embed-text  # For memory/search features
```

### Verify it works
```bash
ollama run qwen2.5:7b "Hello, who are you?"
```

---

## Step 4: Install OpenClaw (10 minutes)

### Prerequisites
```bash
# Node.js 20+ required
node --version  # Should be v20+

# If not installed:
# Linux: curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash - && sudo apt install -y nodejs
# macOS: brew install node
# Windows: Download from nodejs.org
```

### Install OpenClaw
```bash
npm install -g openclaw@latest
```

### Run onboarding
```bash
openclaw onboard
```

During onboarding:
- **Model**: Select "Ollama" and enter your model name (e.g., `qwen2.5:7b`)
- **Channels**: Start with WebChat only (easiest)
- **Workspace**: Accept default or choose a folder

### Start the gateway
```bash
openclaw gateway
```

Open WebChat at: `http://localhost:18789`

---

## Step 5: Hand Off to Your AI (The Fun Part)

Once OpenClaw is running, paste this into WebChat to let your AI configure itself:

```
Hi! I just set you up with OpenClaw. You're running locally on my machine via Ollama.

Please help me configure you properly. I'd like you to:

1. **Learn about me**: Ask me 5-10 questions about:
   - What I do (work, hobbies, projects)
   - How I want to use you (tasks, reminders, research, coding, etc.)
   - My communication preferences (brief vs detailed, formal vs casual)
   - Any tools or services I use regularly
   - What I should call you (pick a name you like!)

2. **Set up your identity**: Based on my answers, create a SOUL.md file that defines who you are and how you'll help me.

3. **Configure your memory**: Create a USER.md with what you learned about me.

4. **Suggest next steps**: What tools or integrations would help me most?

Let's start — what would you like to know about me?
```

Your AI will interview you and write its own configuration files. This is the magic of OpenClaw — the AI helps set itself up.

---

## Step 6: Add Cloud AI for Complex Tasks (Optional but Recommended)

### Why Hybrid is Best

**Local models are great for:**
- ✅ Privacy-sensitive queries
- ✅ Quick simple tasks
- ✅ Offline access
- ✅ No API costs for casual use

**Cloud models (Anthropic Claude, OpenAI) are better for:**
- ✅ Complex reasoning and analysis
- ✅ Long documents and research
- ✅ Coding assistance
- ✅ Tasks requiring large context windows

**The cost reality:**
- Anthropic API: ~$3/million input tokens, ~$15/million output tokens
- Typical daily use: $0.10-0.50/day
- A $20 credit lasts most people 1-2 months

### Add Anthropic (Recommended)

1. Get API key: https://console.anthropic.com/
2. Run: `openclaw onboard` and add your key when prompted
3. Or edit `~/.openclaw/openclaw.json`:

```json
{
  "agents": {
    "defaults": {
      "model": "anthropic/claude-sonnet-4-20250514"
    }
  },
  "auth": {
    "anthropic": {
      "default": { "apiKey": "sk-ant-..." }
    }
  }
}
```

### Hybrid Setup: Local Default, Cloud When Needed

```json
{
  "agents": {
    "defaults": {
      "model": "ollama/qwen2.5:7b"  // Local for most things
    }
  }
}
```

Then in chat, use `/model anthropic/claude-sonnet-4` to switch when you need power.

Or configure specific agents:
```json
{
  "agents": {
    "list": [
      { "id": "local", "model": "ollama/qwen2.5:7b" },
      { "id": "power", "model": "anthropic/claude-sonnet-4-20250514" }
    ]
  }
}
```

---

## Security: When to Use Local vs Cloud

### Use LOCAL for:
- Personal journal/diary entries
- Health information
- Financial details
- Passwords and credentials (don't share these with ANY AI)
- Private conversations
- Anything you wouldn't email

### Use CLOUD for:
- General research
- Public information processing
- Coding help (non-proprietary)
- Writing assistance
- Complex analysis

### The Privacy Reality

**Local models:**
- Data never leaves your machine ✅
- No logging by third parties ✅
- You control everything ✅

**Cloud models (Anthropic):**
- Data sent to their servers ⚠️
- Anthropic doesn't train on API data (as of 2025)
- Still subject to their privacy policy
- Required for complex tasks

**Best practice:** Default to local, switch to cloud consciously when you need the power.

---

## Quick Reference

### Commands
```bash
openclaw gateway          # Start the gateway
openclaw gateway status   # Check status
openclaw doctor           # Diagnose issues
openclaw channels list    # See connected channels
ollama list               # See installed models
ollama pull MODEL         # Download a model
```

### Files
```
~/.openclaw/
├── openclaw.json         # Main config
└── workspace/
    ├── SOUL.md           # AI's identity
    ├── USER.md           # About you
    ├── AGENTS.md         # Operating instructions
    └── MEMORY.md         # Long-term memory
```

### Get Help
- Docs: https://docs.openclaw.ai
- GitHub: https://github.com/openclaw/openclaw
- Discord: https://discord.com/invite/clawd

---

## TL;DR

1. Run the spec command, paste output into any AI
2. Install Ollama + recommended model
3. `npm install -g openclaw && openclaw onboard`
4. Open WebChat, paste the setup prompt, let your AI interview you
5. Optionally add Anthropic API for complex tasks ($0.10-0.50/day)

**Time to first chat: ~20 minutes**

---

*Created: 2026-02-03*
