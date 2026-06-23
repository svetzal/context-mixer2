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

## Managed platforms

By default cmx **infers** which platforms to act on: a bare `install` targets the
platforms already in use, while `uninstall` and `doctor` consider every supported
platform. Declare a managed set to make that explicit and authoritative — when it
is non-empty, a default (no `--platform`) `install`/`uninstall` acts on exactly
those platforms and `doctor` surveys only those:

```bash
cmx config platforms add claude
cmx config platforms add codex     # cmx now manages exactly claude + codex
cmx config platforms list
cmx config platforms remove codex
```

The set is stored as lowercase names in `config.json` (`"platforms": ["claude",
"codex"]`) and shown by `cmx config show` (as `(inferred)` when unset). This is
the clean way to onboard a tool before its first install, and to stop cmx from
scanning tools you don't use. An explicit `--platform` still overrides the set
for a single command. See the
[command reference](../reference/commands.md#managed-platforms) for details.

## External rules

Artifacts another tool manages — e.g. a tool's bundled/stock skills in its own
directory — can be declared *external* so `cmx doctor` reports them as `external`
(a steady state, not flagged) instead of as orphaned, and so `adopt` never sweeps
them into your home:

```bash
cmx config external add ~/.hermes/skills   # a whole tool's skill directory
cmx config external add some-skill         # a single artifact by name
cmx config external list
cmx config external remove some-skill
```

Each rule is either a **directory** (an install location — `~` expands to your
home, and it covers everything under it) or a bare **artifact name** (matched
wherever it lives). See
[Bringing a System Under Control](./under-control.md) for how external rules fit
the adoption workflow.

## Environment variables

| Variable | Purpose |
|----------|---------|
| `OPENAI_API_KEY` | API key for OpenAI gateway |
| `OLLAMA_HOST` | Ollama server URL (default: `http://localhost:11434`) |
