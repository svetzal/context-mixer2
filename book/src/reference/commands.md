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
| `cmx agent adopt <name>` | Adopt an orphaned, hand-authored agent into the canonical home |
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
| `cmx doctor --adopt-all` | Adopt every orphaned artifact into the canonical home |

### `cmx doctor`

`doctor` is a **read-only** survey across *every* supported platform's install
directories and lock files. It mutates nothing — its job is to make a
disorganized installation visible before any command changes it. For each
artifact it reports one of:

| State | Meaning | Remedy |
|-------|---------|--------|
| `tracked` | recorded in a lock file with a matching checksum | — |
| `drifted` | tracked, but the on-disk copy was edited after install | `cmx info <name>` to inspect |
| `untracked` | on disk, no lock entry, **but a registered source provides it** (installed out-of-band) | `cmx <kind> install <name>` to track it |
| `orphaned` | on disk, no lock entry, and **no source provides it** (hand-authored) | `cmx <kind> adopt <name>` to canonicalize into the home |
| `missing` | in a lock file, but the file is gone from disk | `cmx <kind> uninstall <name>` to clear the stale entry |

The `untracked` vs `orphaned` split matters for bringing a system under control:
*untracked* artifacts have a real upstream source, so the right move is to track
them (`install`); *orphaned* artifacts are yours alone, so they belong in the
canonical home (`adopt`). `cmx doctor --adopt-all` and `cmx <kind> adopt <name>`
therefore act **only on orphaned** artifacts — an untracked artifact is steered
to `install` instead of being adopted as if it were private.

It also flags artifacts of the same name appearing in more than one distinct
install location (`(dup)`). Skills in the shared `.agents/skills` directory that
many tools read are reported **once**, attributed to the whole cohort — not as
duplicates.

`doctor` exits non-zero (`2`) when it finds drift, orphans, or missing entries,
so it is usable in a pre-commit hook or CI check. Cross-location duplication
alone does not fail it — projecting one curated set into many tools legitimately
produces copies.

## Canonical home & adoption

The **canonical home** is a tool-neutral directory that holds your hand-authored
private agents and skills — the source of truth that survives switching coding
assistants. It defaults to `~/.config/context-mixer/home` (override with the
`home` field in `config.json`) and is auto-registered as a visible local source
named `home`.

| Command | Description |
|---------|-------------|
| `cmx home init` | Create the home directory and register it as the `home` source |
| `cmx home path` | Print the resolved home directory |
| `cmx skill adopt <name>...` | Adopt one or more named orphaned skills into the home |
| `cmx agent adopt <name>...` | Adopt one or more named orphaned agents into the home |
| `cmx skill adopt --all [--from <dir>]` | Adopt all orphaned skills, optionally only those under `<dir>` |
| `cmx doctor --adopt-all [--from <dir>]` | Adopt every orphan the survey finds (both kinds), optionally scoped to `<dir>` |

Adoption acts **only on orphaned** artifacts. Naming an untracked
(source-available) artifact steers you to `install`; an already-tracked or
drifted one is rejected. Named adoption is all-or-nothing — if any name is
invalid, nothing is adopted. The `--from <dir>` filter restricts a bulk adopt to
a single install location (e.g. `--from ~/.claude/skills` to adopt your own
skills while leaving another tool's bundled-skill directory untouched).

**Adoption copies, never moves.** It places a verbatim copy of the orphan in the
home, registers the home as a source, and records provenance (`source: home`,
with the artifact's checksum) in the lock file of every platform that reads the
orphan's location — so the original reclassifies from *orphaned* to *tracked*.
The original file is left exactly where it was.

### Migrating a private skill set between tools

```text
cmx doctor                 # see what's orphaned
cmx doctor --adopt-all     # canonicalize the orphaned private artifacts
cmx skill install --all --platform opencode   # project the home to a new tool
```

After adoption the home is a normal source, so projecting it to any platform is
just `install --all --platform <tool>`.

## Configuration

| Command | Description |
|---------|-------------|
| `cmx config show` | Show current LLM settings |
| `cmx config gateway <openai\|ollama>` | Set LLM provider |
| `cmx config model <name>` | Set LLM model |
