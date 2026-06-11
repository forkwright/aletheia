# episteme

Knowledge pipeline for Aletheia. Extraction, storage, recall, and maintenance
of the knowledge graph.

## Embedding providers

### OpenAI-compatible HTTP provider

Enable the `openai-embed` feature and set `provider = "openai-compat"` in the
embedding configuration. This offloads embedding inference to any endpoint that
implements the OpenAI `/v1/embeddings` surface - OpenAI, Voyage, Cohere (with a
shim), or a local **llama-server**.

```toml
[embedding]
provider = "openai-compat"
base_url = "http://127.0.0.1:5005/v1"
model = "qwen-embed"
dimension = 384
```

For a local embedding service, run Qwen3-Embedding-0.6B or another compatible
embedding model at port `5005` with an OpenAI shim (`llama-server --embedding`).
Pointing `base_url` at
`http://127.0.0.1:5005/v1` keeps weights in a single process, eliminating the
~2GB duplicate VRAM load that occurs when candle runs in-process alongside the
inference server. For other local or cloud deployments, point `base_url` at the
OpenAI-compatible embedding endpoint you operate.

For authenticated cloud endpoints, add an `api_key`:

```toml
[embedding]
provider = "openai-compat"
base_url = "https://api.openai.com/v1"
model = "text-embedding-3-small"
dimension = 1536
api_key = "sk-..."
```
