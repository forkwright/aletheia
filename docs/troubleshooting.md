# Troubleshooting

Common issues and fixes. For setup, see [QUICKSTART.md](QUICKSTART.md). For config reference, see [CONFIGURATION.md](CONFIGURATION.md). For production deployment, see [DEPLOYMENT.md](DEPLOYMENT.md).

---

| Problem | Fix |
|---------|-----|
| `ANTHROPIC_API_KEY not set` | Export the env var or add to systemd `Environment=` |
| Port already in use | `fuser -k 18789/tcp` then restart, or change `gateway.port` in config |
| Config parse error | Check YAML syntax, verify field names match [CONFIGURATION.md](CONFIGURATION.md) |
| Health returns `degraded` | No LLM provider registered; check API key |
| Health returns `unhealthy` | Session store failed to open; check `instance/data/` permissions |
| Signal not receiving | Verify signal-cli daemon is running on configured host:port |
| Bind address error | Check `--bind` flag or `gateway.bind` config; `lan` resolves to LAN interface |
