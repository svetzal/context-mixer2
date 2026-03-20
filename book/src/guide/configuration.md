# Configuration

cmx stores configuration in `~/.config/context-mixer/config.json`.

## View current settings

```bash
cmx config show
```

## LLM Gateway

The `diff` command uses an LLM for analysis. By default, cmx uses OpenAI with the `gpt-5.4` model.

### Set the gateway

```bash
# OpenAI (default) — uses $OPENAI_API_KEY
cmx config gateway openai

# Ollama (local)
cmx config gateway ollama
```

### Set the model

```bash
# OpenAI models
cmx config model gpt-5.4

# Ollama models
cmx config gateway ollama
cmx config model qwen3.5:27b
```

## Environment variables

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | API key for OpenAI gateway |
| `OLLAMA_HOST` | Ollama server URL (default: `http://localhost:11434`) |
