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
| `cmx doctor` | Survey the whole system installation across every platform (read-only) |
| `cmx doctor --local` | Also include project (local) scope in the survey |

### `cmx doctor`

`doctor` is a **read-only** survey across *every* supported platform's install
directories and lock files. It mutates nothing — its job is to make a
disorganized installation visible before any command changes it. For each
artifact it reports one of:

| State | Meaning |
|-------|---------|
| `tracked` | recorded in a lock file with a matching checksum |
| `drifted` | tracked, but the on-disk copy was edited after install |
| `orphaned` | on disk with no lock entry on any platform (e.g. hand-authored) |
| `missing` | in a lock file, but the file is gone from disk |

It also flags artifacts of the same name appearing in more than one distinct
install location (`(dup)`). Skills in the shared `.agents/skills` directory that
many tools read are reported **once**, attributed to the whole cohort — not as
duplicates.

`doctor` exits non-zero (`2`) when it finds drift, orphans, or missing entries,
so it is usable in a pre-commit hook or CI check. Cross-location duplication
alone does not fail it — projecting one curated set into many tools legitimately
produces copies.

## Configuration

| Command | Description |
|---------|-------------|
| `cmx config show` | Show current LLM settings |
| `cmx config gateway <openai\|ollama>` | Set LLM provider |
| `cmx config model <name>` | Set LLM model |
