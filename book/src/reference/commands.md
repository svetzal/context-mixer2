# Command Reference

## Global options

| Option | Description |
|--------|-------------|
| `--platform <platform>` | Target platform: `claude` (default), `copilot`, `cursor`, `windsurf`, `gemini` |

The `--platform` flag is global and can be placed anywhere on the command line.
It can also be set via the `CMX_PLATFORM` environment variable.

## Source management

| Command | Description |
|---------|-------------|
| `cmx source add <name> <path-or-url>` | Register a marketplace source |
| `cmx source list` | List registered sources |
| `cmx source browse <name>` | Show available artifacts in a source |
| `cmx source update [name]` | Fetch latest for git sources (all if no name) |
| `cmx source remove <name>` | Unregister and clean up clone |

## Agent management

| Command | Description |
|---------|-------------|
| `cmx agent install <name>` | Install an agent from sources |
| `cmx agent install <source>:<name>` | Install from a specific source |
| `cmx agent install --all` | Install all available agents |
| `cmx agent install <name> --local` | Install into current project |
| `cmx agent install <name> --platform cursor` | Install to Cursor |
| `cmx agent update <name>` | Update an agent from its source |
| `cmx agent update --all` | Update all tracked agents |
| `cmx agent uninstall <name>` | Uninstall an agent |
| `cmx agent list` | List installed agents |
| `cmx agent diff <name>` | LLM-powered diff analysis (requires `llm` feature) |

## Skill management

Same commands as agent, using `cmx skill` instead of `cmx agent`.

## Aggregate commands

| Command | Description |
|---------|-------------|
| `cmx list` | List all installed agents and skills |
| `cmx outdated` | Show artifacts needing attention |
| `cmx search <keyword>` | Search all sources by name and description |
| `cmx info <name>` | Show detailed metadata for an installed artifact |

## Configuration

| Command | Description |
|---------|-------------|
| `cmx config show` | Show current LLM settings |
| `cmx config gateway <openai\|ollama>` | Set LLM provider |
| `cmx config model <name>` | Set LLM model |
