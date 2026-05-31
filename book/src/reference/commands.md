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
| `cmx agent install <name>...` | Install one or more agents from sources |
| `cmx agent install <source>:<name>` | Install from a specific source |
| `cmx agent install --all` | Install all available agents |
| `cmx agent install <name> --local` | Install into current project |
| `cmx agent install <name> --platform cursor` | Install to Cursor |
| `cmx agent update <name>` | Update an agent from its source |
| `cmx agent update --all` | Update all tracked agents |
| `cmx agent uninstall <name>...` | Uninstall one or more agents (everywhere cmx tracks them) |
| `cmx agent adopt <name>...` | Adopt one or more orphaned, hand-authored agents into the canonical home |
| `cmx agent unadopt <name>...` | Remove agents from the home (originals stay); `--external` also marks them external |
| `cmx agent list` | List installed agents (cmx-managed); `--all` includes external |
| `cmx agent diff <name>` | LLM-powered diff analysis (requires `llm` feature) |

`install` and `uninstall` accept **multiple names** in one command (e.g.
`cmx agent install a b c`). Both are best-effort: each name is processed
independently and per-name failures are collected and reported rather than
aborting the batch. `install` exits non-zero if any name failed; `uninstall`
exits non-zero only when nothing at all was removed. `uninstall` is
**cross-platform** — it removes every physical copy and clears every platform's
lock entry, not just the active `--platform`'s.

## Skill management

Same commands as agent, using `cmx skill` instead of `cmx agent`.

## Aggregate commands

| Command | Description |
|---------|-------------|
| `cmx list` | List installed agents and skills (cmx-managed inventory) |
| `cmx list --all` | Include external (tool-managed) artifacts in the listing |
| `cmx outdated` | Show artifacts needing attention |
| `cmx search <keyword>` | Search all sources by name and description |
| `cmx info <name>` | Show detailed metadata for an installed artifact |
| `cmx doctor` | Survey every platform; show only what needs attention (read-only) |
| `cmx doctor --all` | Show the full inventory, not just problems |
| `cmx doctor --local` | Also include project (local) scope in the survey |
| `cmx doctor --adopt-all` | Adopt every orphaned artifact into the canonical home |

### `cmx doctor`

`doctor` is a **read-only** survey across *every* supported platform's install
directories and lock files. It mutates nothing — its job is to make a
disorganized installation visible before any command changes it.

By default `doctor` shows **only what needs attention** — drifted, untracked,
orphaned, missing, or diverged artifacts — because it's a doctor, for fixing
broken things. Healthy `tracked` and `external` artifacts are counted in the
summary line but not listed. Pass **`--all`** for the full inventory. When
nothing's wrong it reports "everything cmx manages is healthy." For each
artifact it reports one of:

| State | Meaning | Remedy |
|-------|---------|--------|
| `tracked` | recorded in a lock file with a matching checksum | — (unless diverged) |
| `drifted` | tracked, but the on-disk copy was edited after install | `cmx info <name>` to inspect |
| `untracked` | on disk, no lock entry, **but a registered source provides it** (installed out-of-band) | `cmx <kind> install <name>` to track it |
| `orphaned` | on disk, no lock entry, and **no source provides it** (hand-authored) | `cmx <kind> adopt <name>` to canonicalize into the home |
| `external` | on disk, but declared external in config (managed by another tool) | — (unless diverged) |
| `missing` | in a lock file, but the file is gone from disk | `cmx <kind> uninstall <name>` to clear the stale entry |

The `untracked` vs `orphaned` split matters for bringing a system under control:
*untracked* artifacts have a real upstream source, so the right move is to track
them (`install`); *orphaned* artifacts are yours alone, so they belong in the
canonical home (`adopt`). `cmx doctor --adopt-all` and `cmx <kind> adopt <name>`
therefore act **only on orphaned** artifacts — an untracked artifact is steered
to `install` instead of being adopted as if it were private.

A skill installed for several tools is reported as **one logical artifact**
whose `Tools` column lists every tool it's installed for — not as N duplicates.
That's the intended "curate once, project to many" outcome. The only
multi-location situation `doctor` flags is `(diverged)`: copies that actually
**disagree** — a different version or state across locations. A divergence is an
anomaly worth surfacing *whoever* owns the artifact, so it's flagged even for
`external` artifacts; cmx just can't be the one to re-sync an external one (its
owning tool must). When versions differ the `Version` column names the skew
(e.g. `3.2.0 / 3.3.0`) instead of collapsing to `-`, and a detail line under the
summary names which copy is where:

```text
  • hopper-coordinator diverges: ~/.agents/skills @ 3.2.0, ~/.claude/skills @ 3.3.0
```

Re-sync a cmx-managed divergence with `cmx <kind> update <name> --force`.

`doctor` exits non-zero (`2`) when it finds drift, untracked, orphaned, missing,
or diverged artifacts, so it is usable in a pre-commit hook or CI check. A
*consistent* `tracked` or `external` artifact never fails it — only a genuine
anomaly does.

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
| `cmx config show` | Show current LLM settings and external rules |
| `cmx config gateway <openai\|ollama>` | Set LLM provider |
| `cmx config model <name>` | Set LLM model |
| `cmx config external list` | List the configured external rules |
| `cmx config external add <dir-or-name>` | Mark a directory or artifact name as external |
| `cmx config external remove <dir-or-name>` | Remove an external rule |

### External artifacts

Artifacts that **another tool manages** — e.g. a tool's bundled/stock skills in
its own directory — can be declared *external* so `cmx doctor` reports them as
`external` (a steady state, not flagged) instead of as orphaned, and so
`adopt`/`--adopt-all` never sweep them into your home. The one exception is a
**divergence**: if an external artifact's copies disagree across locations,
`doctor` still surfaces it — that's a real anomaly even though its owning tool,
not cmx, must re-sync it.

Each rule is either a **directory** (an install location — `~` expands to your
home) or a bare **artifact name**:

```bash
cmx config external add ~/.hermes/skills   # a whole tool's skill directory
cmx config external add some-skill         # a single artifact by name
```

A directory rule covers everything under it (including artifacts added later); a
name rule matches that artifact wherever it lives.
