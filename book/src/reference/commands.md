# Command Reference

## Global options

| Option | Description |
|--------|-------------|
| `--platform <platform>` | Target a single platform: `claude`, `copilot`, `cursor`, `windsurf`, `gemini`, `opencode`, `codex`, `pi`, `crush`, `amp`, `zed`, `openhands`, `hermes` |

The `--platform` flag is global and can be placed anywhere on the command line.
It can also be set via the `CMX_PLATFORM` environment variable.

When omitted, `install` and `uninstall` act across **every platform already in
use** (those with tracked artifacts) — so a curated set stays in sync across the
tools you actually run — while single-target commands (`info`, `update`,
`adopt`, …) default to Claude. Pass `--platform <tool>` to constrain an
operation to one platform (which also onboards a new tool on install). See
[Platform Paths](./platforms.md) for the full directory table, and
`cmx config platforms` (below) to make the managed set explicit.

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
| `cmx agent update <name>` | Update an agent from its source (pulls the source over the installed copy) |
| `cmx agent update --all` | Update all tracked agents |
| `cmx agent promote <name>` | Push in-place edits of the installed copy back into the canonical home — the mirror of `update` |
| `cmx agent uninstall <name>...` | Uninstall one or more agents; sweeps every platform unless `--platform` is given |
| `cmx agent adopt <name>...` | Adopt one or more orphaned, hand-authored agents into the canonical home |
| `cmx agent unadopt <name>...` | Remove agents from the home (originals stay); `--external` also marks them external |
| `cmx agent list` | List installed agents (cmx-managed); `--all` includes external |
| `cmx agent info <name>` | Show source, version, activation trigger, and (in an `llm` build) a summary |
| `cmx agent diff <name>` | Directional diff vs the source; `--full` for the line-by-line view (requires `llm` feature) |

`install` and `uninstall` accept **multiple names** in one command (e.g.
`cmx agent install a b c`). Both are best-effort: each name is processed
independently and per-name failures are collected and reported rather than
aborting the batch. `install` exits non-zero if any name failed; `uninstall`
exits non-zero only when nothing at all was removed.

Both are **multi-platform by default**. With no `--platform`, a bare `install`
lands the artifact on every platform already in use (falling back to Claude when
nothing is tracked yet), and `uninstall` sweeps every platform — removing each
physical copy and clearing every platform's lock entry. Pass `--platform <tool>`
to scope either to one platform: install onboards just that tool, uninstall
removes from just that one and leaves the others intact.

`update` is intentionally different: without `--platform`, it targets only the
default platform (Claude). `cmx <kind> update --all` means all tracked artifacts
on that one platform, not all platforms.

## Skill management

Same commands as agent, using `cmx skill` instead of `cmx agent`, plus one
skill-only command:

| Command | Description |
|---------|-------------|
| `cmx skill sync <name>` | Reconcile a skill that has diverged across platforms by copying one copy over the others |

`sync` works **between install locations** rather than from a registered source,
so it also reconciles `external` skills and any skill with no source. By default
the **newest version wins**; pass `--from <platform>` to force the direction,
`--dry-run` to preview, or `--local` to reconcile project scope. When the
differing copies are unversioned (or share a version) it asks for `--from`
rather than guessing, and lists each copy so you can choose. Agents are excluded
because they're reformatted per platform (e.g. Codex TOML), so a byte-level
cross-platform comparison isn't meaningful.

Use `sync` when you want every installed copy of a skill to match. `update`
pulls from source to one platform; `promote` pushes an installed copy back into
the canonical home.

### `cmx {agent,skill} promote`

`promote` is the mirror of `update`. Where `update` pulls the source copy over
your installed one (discarding local edits), `promote` copies the **installed**
copy into the canonical home and refreshes its `home`-provenance lock baselines,
so the artifact reads as tracked again. Use it for the common authoring loop: an
assistant edits its own skill where it's installed, then you promote those edits
to the home. It promotes the copy `cmx diff` shows (global scope preferred, then
project). The target is the home only — a git-sourced artifact is rejected with
guidance (edit the source clone, or `update --force` to discard), as is an
untracked one (steered to `adopt`/`install`); a Codex agent is rejected too,
since its installed copy is TOML rather than canonical markdown. Any other
home-tracked platform whose copy still differs afterward is reported as drifted
and pointed at `sync`.

## Aggregate commands

| Command | Description |
|---------|-------------|
| `cmx list` | List installed agents and skills (cmx-managed inventory) |
| `cmx list --all` | Include external (tool-managed) artifacts in the listing |
| `cmx outdated` | Show artifacts needing attention |
| `cmx search <keyword>` | Search all sources by name and description |
| `cmx info <name>` | Show detailed metadata for an installed artifact (searches both kinds) |
| `cmx completions <shell>` | Generate a shell completion script to stdout |
| `cmx doctor` | Survey every platform; show only what needs attention (read-only) |
| `cmx doctor --all` | Show the full inventory, not just problems |
| `cmx doctor --local` | Also include project (local) scope in the survey |
| `cmx doctor --json` | Emit the survey as machine-readable JSON to stdout |
| `cmx doctor --adopt-all` | Adopt every orphaned artifact into the canonical home (deprecated; use `cmx <kind> adopt --all`) |
| `cmx init` | Install cmx's own companion agent skill (global scope by default) |

### `cmx completions`

`cmx completions <shell>` generates a completion script to **stdout** only, so
you can pipe or redirect it wherever your shell expects it. Supported values
are `bash`, `zsh`, `fish`, `elvish`, and `powershell`. This command is
read-only and does not support `--json`.

Examples:

```bash
cmx completions zsh > ~/.zfunc/_cmx
cmx completions bash | sudo tee /etc/bash_completion.d/cmx >/dev/null
```

### `cmx info`

`cmx info <name>` shows the key details of an installed artifact: its scope and
path, version (and any available update), source provenance and checksums, and —
for a skill — its file tree. Two fields are worth calling out:

- **Activates when** (skills) / **Description** (agents) — the artifact's
  `description` frontmatter. For a skill this is precisely its *activation
  trigger*: the "use this when…" text the assistant reads to decide whether to
  load the skill. Multi-line YAML (`description: >` folded or `|` literal) is
  rendered in full.
- **What it does** — a short LLM-generated paragraph summarizing the artifact,
  produced via the configured LLM gateway (`cmx config gateway`/`model`). This
  requires a build with the **`llm` feature**; a lean build prints a one-line
  hint in its place. Summary generation is best-effort — if the provider is
  unreachable, `info` still prints everything else.

The top-level `cmx info <name>` searches both kinds; the kind-scoped
`cmx skill info <name>` and `cmx agent info <name>` look only at that kind.

## Sets

A **set** is a locally-defined, named group of installed artifacts (agents
and/or skills) with a desired activation state — `active` or `inactive`. It is
the consumer-side lever for standing context cost: an installed skill's
trigger description is loaded on every turn whether or not it fires, so a
machine curated to fifty skills carries fifty descriptions of fixed cost.
Uninstalling to reclaim that cost loses the record that you wanted the
artifact at all; a set lets you switch a whole group off and back on as a
unit, fully reversible, with the grouping remembered. See
[Managing Context Cost with Sets](../guide/sets.md) for the task-oriented
walkthrough.

```text
cmx set create <name> [--desc <text>] [--from-plugin <source>:<plugin>] [--local]
cmx set list [--local] [--json]
cmx set show <name> [--local] [--json]
cmx set add <name> <artifact>... [--local]
cmx set remove <name> <artifact>... [--local]
cmx set activate <name> [--apply] [--local]
cmx set deactivate <name> [--apply] [--force] [--local]
cmx set delete <name> [--purge [--apply] [--force]] [--local]
cmx set rename <old> <new> [--local]
```

Every subcommand defaults to **global** scope; pass `--local` to operate on a
project-scoped set instead (`.context-mixer/sets.json`, symmetric with the
local lock file). `create`, `add`, `remove`, `activate`, `deactivate`,
`delete`, and `rename` are the mutating verbs (marked `[Mutates]` in
`--help`); `list` and `show` are read-only and support `--json`.

| State | Meaning |
|-------|---------|
| `active` | Every member is installed; `cmx set activate` made it so |
| `inactive` | Members are not installed on the set's behalf; either never activated, or `deactivate` uninstalled them |

`activate` installs every member from its pinned source into the normally
resolved install targets and marks the set active — it is **idempotent**, so
re-running it safely repairs a set that's only partially installed.
`deactivate` uninstalls every member **not held by another active set** and
marks the set inactive. Deactivating does not delete the set's definition —
only `delete` does that — so a deactivated set can always be reactivated
later without rediscovering its members.

### Plan-by-default, `--apply` to execute

`activate` and `deactivate` (and `delete --purge`) show the concrete plan —
which members would be installed or uninstalled, and where — without touching
disk. Pass `--apply` to execute exactly that plan:

```text
$ cmx set activate rust-work
Plan to activate set 'rust-work':
  agent rust-craftsperson: install
    ~/guidelines/plugins/rust-ecosystem/agents/rust-craftsperson.md -> ~/.claude/agents/rust-craftsperson.md
Re-run with --apply to make these changes.

$ cmx set activate rust-work --apply
Set 'rust-work' activated.
  agent rust-craftsperson: installed
    ~/guidelines/plugins/rust-ecosystem/agents/rust-craftsperson.md -> ~/.claude/agents/rust-craftsperson.md
```

### Reference counting across sets

An artifact can belong to more than one set. `deactivate` only uninstalls a
member when **no other active set** still claims it — the same rule
`cmx doctor`'s set-consistency check applies when deciding whether a lingering
install is a problem. Deactivating one set therefore never yanks an artifact
out from under a different set that's still active and needs it.

### The drift guard

A member with local edits (installed copy differs from what cmx tracked)
blocks its own uninstall during `deactivate` — cmx won't silently discard
edited content. Pass `--force` to discard the drifted copy and uninstall it
anyway. The same guard and `--force` override apply to `delete --purge`,
since a purge is a deactivate followed by deleting the definition.

### Seeding from a marketplace plugin

`cmx set create <name> --from-plugin <source>:<plugin>` seeds the new set's
membership from a marketplace plugin's declared `agents`/`skills` (its
`marketplace.json` entry) — without installing anything. This is the
publisher-side plugin grouping handed to you as a starting point for a
consumer-side set; from there the set is yours to add/remove members from
regardless of which source or plugin they came from.

### `delete --purge`

`cmx set delete <name>` removes only the set's definition; its members stay
installed (they may belong to something else). `--purge` also deactivates the
set first — uninstalling any member not held by another active set — before
deleting it. Like `activate`/`deactivate`, the purge is previewed by default
and only executed with `--apply`; `--force` applies the same drift override.

### `cmx set list --json` / `cmx set show --json`

```json
{
  "scope": "global",
  "sets": [
    { "name": "rust-work", "state": "active", "member_count": 1, "footprint_chars": 1800 }
  ]
}
```

```json
{
  "description": "Rust craftsmanship + foundry",
  "footprint_chars": 1800,
  "members": [
    {
      "footprint_chars": 1800,
      "installed": true,
      "kind": "agent",
      "name": "rust-craftsperson",
      "source": "guidelines"
    }
  ],
  "name": "rust-work",
  "scope": "global",
  "state": "active"
}
```

`footprint_chars` is the total character count of the set's members' trigger
descriptions (the `description` frontmatter an assistant loads on every
turn) — the context-footprint cost the set carries while active. A member
whose description can't be resolved (source missing, artifact not found)
contributes `0` to the set's total and its own `footprint_chars` is `null`
rather than `0`, so a script can tell "counted as zero" apart from
"genuinely couldn't resolve."

### `cmx doctor`

`doctor` is a **read-only** survey of your platforms' install directories and
lock files. It mutates nothing — its job is to make a disorganized installation
visible before any command changes it. By default it surveys every supported
platform; once you declare a managed set with `cmx config platforms`, it surveys
only those, and the header reports the count it actually looked at (e.g. "2
managed platform(s) surveyed") rather than always claiming all fourteen.

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
whose `Platforms` column lists every surveyed copy as `platform@version` — not
as N duplicates. That's the intended "curate once, project to many" outcome.
The only multi-location situation `doctor` flags is `(diverged)`: copies whose
**content differs** across locations. A divergence is an anomaly worth
surfacing *whoever* owns the artifact, so it's flagged even for `external`
artifacts; cmx just can't be the one to re-sync an external one (its owning
tool must). Because the table attributes the version to each platform directly,
it can show equal versions for content-diverged copies (for example
`claude@1.1.2, codex@1.1.2`) or version skew (`codex@3.2.0, claude@3.3.0`)
without hiding which copy is where. A detail line under the summary still names
the paths:

```text
  • hopper-coordinator diverges: ~/.agents/skills @ 3.2.0, ~/.claude/skills @ 3.3.0
```

Re-sync a divergence with the tool that fits its provenance. Doctor's hint is
case-directed so you can pick the right command by situation:

- source- or home-backed, edited in place → `cmx skill promote <name>`
- source-backed, restore from source → `cmx skill update <name> --force`
- external / source-less → `cmx skill sync <name>` (or `--from <platform>`)
- not sure? inspect first → `cmx skill diff <name>`

#### Set-consistency checks

`doctor` also cross-references every [set](#sets)'s declared membership
against what the survey found installed, at every scope it surveys. Two
mismatches are flagged:

| Problem | Meaning | Remedy |
|---------|---------|--------|
| `active_missing` | The set is `active`, but this member isn't installed | `cmx set activate <name>` repairs it (idempotent) |
| `inactive_lingering` | The set is `inactive`, but this member is still installed and not held by any other `active` set | `cmx set deactivate <name>` — previews first, clears it with `--apply` |

The same reference-counting rule `deactivate` itself uses applies here: a
member shared by two sets is never flagged `inactive_lingering` while either
set is still active. In the human view each mismatch appears as its own
detail line under the summary:

```text
  • set 'rust-work' (global): agent rust-craftsperson is active but not installed
```

#### Exit codes

`doctor` exits non-zero (`2`) when it finds drift, untracked, orphaned,
missing, or diverged artifacts, **or a set-consistency mismatch**, so it is
usable in a pre-commit hook or CI check. A *consistent* `tracked` or
`external` artifact never fails it — only a genuine anomaly does. This holds
for both the human and `--json` output:

| Exit code | Meaning |
|-----------|---------|
| `0` | no issues found |
| `2` | actionable issues found (drifted, untracked, orphaned, missing, or diverged artifacts, or a set-consistency mismatch) |

#### `cmx doctor --json`

`--json` prints the survey as a single JSON document to **stdout only** — no
human table, no prose. Any warnings (like the `--adopt-all` deprecation notice
below) still go to stderr, so `cmx doctor --json | jq .` always sees clean JSON.
The shape mirrors the human view:

```json
{
  "scope": "global",
  "platforms_surveyed": 13,
  "showing": "needs_attention",
  "summary": {
    "tracked": 40, "drifted": 1, "untracked": 0,
    "orphaned": 1, "external": 8, "missing": 0, "diverged": 4,
    "set_inconsistent": 1
  },
  "artifacts": [
    {
      "kind": "skill",
      "name": "hopper-coordinator",
      "scope": "global",
      "state": "external",
      "versions": ["3.2.0", "3.3.0"],
      "source": null,
      "tools": [],
      "diverged": true,
      "locations": [
        { "path": "~/.agents/skills", "platform": "codex", "version": "3.2.0", "state": "external" },
        { "path": "~/.claude/skills", "platform": "claude", "version": "3.3.0", "state": "external" }
      ]
    }
  ],
  "set_inconsistencies": [
    {
      "set": "rust-work",
      "scope": "global",
      "kind": "agent",
      "member": "rust-craftsperson",
      "problem": "active_missing"
    }
  ]
}
```

`showing` reflects the same selection the human table would use — `--all`
switches it from `"needs_attention"` to `"all"` and includes healthy artifacts
too. An artifact carries `version` when every copy agrees, or `versions` (an
array) when they diverge. Every artifact's `locations` array replaces the human
view's free-text "diverges: ..." line with structured `{path, platform,
version, state}` entries, so a script never has to parse prose to find where
copies disagree. `set_inconsistencies` mirrors the set-consistency check above
— `problem` is `"active_missing"` or `"inactive_lingering"`.

### `cmx init`

`cmx init` installs cmx's own companion agent skill — the skill that teaches an
agent to drive `cmx` itself — through the shared `cmx-core` library, the same
embeddable installer other fleet tools (parite, foundry) use for their own
companion skills.

| Flag | Effect |
|------|--------|
| *(none)* | Install/update at **global scope** (`~/.claude/skills/cmx/`) — the default, since a companion skill describes the tool, not one project |
| `--local` | Install into the current project (`.claude/skills/cmx/`) instead |
| `--force` | Overwrite even if the installed copy is a *newer* version than the bundled one (otherwise refused) |
| `--remove` | Uninstall — removes the skill directory and clears its lock entry (leaves the shared `cmx-lock.json` in place) |
| `--json` | Emit a machine-readable report instead of human text — the only `cmx` command that does, today |
| `--global` | Accepted as a no-op alias; global is already the default |

Re-running `cmx init` when the installed copy is already current reports a
skip. `cmx init --global --force` always exits `0`, matching the fleet-wide
registry contract other tools' automation depends on.

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
| `cmx agent adopt --all [--from <dir>]` | Adopt all orphaned agents, optionally only those under `<dir>` |
| `cmx doctor --adopt-all [--from <dir>]` | **Deprecated** — adopt every orphan the survey finds (both kinds), optionally scoped to `<dir>`; prints a stderr warning and will be removed in the next major version. Use `cmx skill adopt --all` / `cmx agent adopt --all` instead. |

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
cmx skill adopt --all      # canonicalize the orphaned private skills
cmx agent adopt --all      # canonicalize the orphaned private agents
cmx skill install --all --platform opencode   # project the home to a new tool
```

After adoption the home is a normal source, so projecting it to any platform is
just `install --all --platform <tool>`.

## Configuration

| Command | Description |
|---------|-------------|
| `cmx config show` | Show current LLM settings, external rules, and the managed-platform set |
| `cmx config gateway <openai\|ollama>` | Set LLM provider |
| `cmx config model <name>` | Set LLM model |
| `cmx config external list` | List the configured external rules |
| `cmx config external add <dir-or-name>` | Mark a directory or artifact name as external |
| `cmx config external remove <dir-or-name>` | Remove an external rule |
| `cmx config platforms list` | List the platforms cmx manages |
| `cmx config platforms add <platform>` | Add a platform to the managed set |
| `cmx config platforms remove <platform>` | Remove a platform from the managed set |

### Managed platforms

By default cmx **infers** which platforms to act on: a bare `install` targets the
platforms already in use, while `uninstall` and `doctor` consider every supported
platform. Declaring a managed set makes that explicit and authoritative — when it
is non-empty, a default (no `--platform`) `install`/`uninstall` acts on exactly
those platforms and `doctor` surveys only those, so cmx ignores tools you don't
use instead of scanning all fourteen:

```bash
cmx config platforms add claude
cmx config platforms add codex     # now cmx manages exactly claude + codex
cmx config platforms list
```

The set is stored as lowercase names in `config.json` (`"platforms": ["claude",
"codex"]`) and shown by `cmx config show` (as `(inferred)` when unset). Onboard a
tool before any install with `cmx config platforms add <tool>`; an explicit
`--platform` still overrides the set for a single command.

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
