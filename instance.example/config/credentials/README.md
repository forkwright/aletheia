# Credentials

Place credential files here. Each file is named for its provider.

## anthropic.json

For OAuth credentials (auto-refreshes):
```json
{
  "token": "your-access-token",
  "refreshToken": "your-refresh-token",
  "expiresAt": 0
}
```

For static API keys:
```json
{
  "token": "sk-ant-api03-your-key-here"
}
```

Run `aletheia credential init` for interactive setup, or `aletheia credential status` to check the current credential source.

## Resolution Order

1. Credential file (`instance/config/credentials/anthropic.json`)
2. Environment variable (`ANTHROPIC_API_KEY`)
