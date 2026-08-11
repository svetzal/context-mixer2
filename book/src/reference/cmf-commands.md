# cmf Command Reference

cmf is the compiler and publisher for intent-based agentic guidance. Run its
commands from the root of an intent library, marketplace, or plugin repository.

## Intents

| Command | Description |
| --- | --- |
| `cmf intent list` | Recursively list TOML intent records by repository-relative key |
| `cmf intent validate` | Validate intent schemas and semantic graph relationships |

Intent keys are paths below `intents/` without the `.toml` extension, such as
`craftsperson/verify-before-declaring-completion`. Relationship targets use the
same key format.

Validation checks that records parse, required authoring fields are present,
confidence is between zero and one, relationship types are recognized, and
every relationship target exists. Errors produce exit code `2`; clean results
and warnings-only results produce exit code `0`.

## Status and aggregate validation

| Command | Description |
| --- | --- |
| `cmf status` | Show repository kind, intent/category counts, plugins, artifacts, and validation summary |
| `cmf validate` | Validate intents, marketplace metadata, and plugins together |

## Plugin management

| Command | Description |
| --- | --- |
| `cmf plugin list` | List plugins with version, category, and artifact counts |
| `cmf plugin init <name>` | Scaffold a plugin directory under `plugins/` |
| `cmf plugin validate` | Validate plugin structures and artifact frontmatter |

`plugin init` requires a marketplace repository. It creates
`plugins/<name>/` with `.claude-plugin/plugin.json`, `agents/`, and `skills/`.

## Marketplace management

| Command | Description |
| --- | --- |
| `cmf marketplace validate` | Check `marketplace.json` against plugin directories |
| `cmf marketplace generate` | Generate or update `marketplace.json` from `plugins/` |

Generation preserves existing entry metadata and adds newly discovered
plugins.

## Manifest generation

| Command | Description |
| --- | --- |
| `cmf manifest generate` | Project canonical `.claude-plugin/` manifests to supported manifest directories |

The generated targets are GitHub Copilot, Cursor, Windsurf, and Gemini CLI.
Platforms without a Claude-style plugin manifest do not receive invented files.
