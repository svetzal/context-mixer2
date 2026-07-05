---
name: cmx
description: >
  Drive cmx, the package manager for curated agentic context (agents and
  skills) across AI coding assistants. Use this skill whenever the user
  mentions cmx, context mixer, installing or updating agent/skill files for
  Claude Code/Copilot/Cursor/Windsurf/Gemini CLI/opencode/Codex CLI/Pi/Crush/
  Amp/Zed/OpenHands/Hermes/Devin, cmx sources, cmx doctor, adopting orphaned
  skills, reconciling drifted skills across tools, or managing a canonical
  home for hand-authored agents/skills. Also use when the user asks "what
  agents/skills do I have installed", "is anything out of date", "add this
  repo as a source", or wants to survey/clean up a messy multi-tool AI
  assistant setup. Even if the user doesn't say "cmx" by name, use this skill
  whenever the task is installing, updating, listing, searching, or auditing
  agent/skill files that AI coding assistants read from disk.
license: MIT
compatibility: Rust binary `cmx`; the `llm`-feature build additionally requires a configured LLM gateway for `info` summaries and `diff`
metadata:
  # Placeholder — stamped to the cmx binary version at install by `cmx init`.
  version: "0.0.0"
  author: Stacey Vetzal
---

# cmx — Context Mixer

cmx installs, updates, and reconciles two kinds of artifacts — **agents** and
**skills** — across every AI coding assistant on the machine (Claude Code,
Copilot, Cursor, Windsurf, Gemini CLI, opencode, Codex CLI, Pi, Crush, Amp,
Zed, OpenHands, Hermes, Devin). Artifacts come from **sources** (a local path
or git repo you register) and are tracked in a lock file so cmx always knows
what's installed, from where, and at what version.

Every command accepts a global `--platform <name>` flag to constrain the
operation to one assistant; when omitted, `install`/`uninstall` act across
every platform already in use, and other commands default to Claude.

## Command grammar (verified against `--help`)

```text
cmx source   {add,list,browse,update,remove}
cmx set      {create,list,show,add,remove,activate,deactivate,delete,rename}
cmx agent    {install,list,info,diff,update,sync,promote,uninstall,unadopt,adopt}
cmx skill    {install,list,info,diff,update,sync,promote,uninstall,unadopt,adopt}
cmx list     [--all] [--json]
cmx doctor   [--local] [--all] [--json]   (deprecated: --adopt-all, --from)
cmx home     {init,path}
cmx outdated [--json]
cmx search   <query> [--json]
cmx info     <name> [--json]
cmx config   {show,gateway,model,external,platforms}
cmx init     [--local] [--force] [--remove] [--json]
```

`agent` and `skill` take the identical set of subcommands — pick the one that
matches what you're managing.

### Sources — where artifacts come from

```bash
cmx source add <name> <path-or-url>   # register a local path or git URL
cmx source list [--json]              # show registered sources
cmx source browse <name> [--json]     # list agents/skills available in a source
cmx source update [<name>]            # git pull registered sources (default: all)
cmx source remove <name>              # unregister (does not delete installed artifacts)
```

### `cmx set` — activation groups for managing context cost

Every *installed* artifact is standing context (its trigger description loads
every turn). A **set** is a named group of artifacts you can install/uninstall
together as a unit, without losing the grouping — the cheaper alternative to
hoarding installs or losing track of what you had.

```bash
cmx set create <name> [--desc <text>] [--from-plugin <source>:<plugin>] [--local]
                                          # --from-plugin seeds membership from a marketplace
                                          # plugin's declared agents/skills (not installed yet)
cmx set list [--json]                    # name, state, member count, context footprint
cmx set show <name> [--json]             # members + per-member source and install status
cmx set add <name> <artifact>...         # snapshot already-installed artifacts into the set
cmx set remove <name> <artifact>...      # drop from set (does NOT uninstall)
cmx set activate <name>   [--dry-run]    # install every member = "turn this set on"
cmx set deactivate <name> [--dry-run] [--force]
                                          # uninstall every member not held by another
                                          # active set = "turn this set off"; remembers
                                          # membership so re-activating reinstalls fresh
cmx set delete <name> [--purge]          # --purge also deactivates first
cmx set rename <old> <new>
```

`activate`/`deactivate` is install/uninstall with remembered membership, not a
separate on-disk state — deactivating an unused set reclaims its context cost
entirely. `cmx doctor` reports set-consistency issues (an active set with
missing members, or an inactive set with lingering installs) alongside its
other checks.

### Install, list, and inspect artifacts

```bash
cmx skill install <name> [<name>...]     # install by name; `source:name` pins a source
cmx skill install --all                  # install everything available
cmx skill install <name> --local         # install into the current project instead of globally
cmx skill install <name> --force         # overwrite even if locally modified
cmx skill list [--all] [--json]          # installed skills (--all includes externally-managed ones)
cmx skill info <name> [--json]           # source, version, activation trigger; a generated
                                          # "what it does" summary too, in an `llm`-feature build
cmx skill uninstall <name> [<name>...] [--local]   # remove everywhere cmx tracks it
```

`cmx agent install/list/info/uninstall` work the same way for agents. There is
no separate plan/dry-run step for these — `install`/`uninstall` write
immediately. `cmx skill sync` is the one artifact subcommand with a
`--dry-run` preview (see below).

### Keeping things current

```bash
cmx outdated [--json]           # installed artifacts with a newer source version
cmx skill update <name>         # pull the latest version from its source
cmx skill update --all          # update every tracked skill
cmx skill update <name> --force # overwrite even if locally modified
```

### Reconciling drift across tools and canonical homes

Skills installed in more than one assistant's directory can diverge (an agent
edits the Claude copy but not the Cursor copy). Three commands handle this:

```bash
cmx skill diff <name> [--full]         # LLM-analyzed comparison, installed vs source
                                        # (requires an `llm`-feature build)
cmx skill sync <name> [--from <tool>] [--dry-run] [--local]
                                        # copy one platform's copy over the others;
                                        # works even for skills cmx doesn't track a
                                        # source for ("external" skills)
cmx skill promote <name> [--from <tool>]
                                        # push in-place edits back into the canonical
                                        # home — the mirror of `update`; if several
                                        # platforms diverge, pick the winner with
                                        # --from
```

### Canonical home and adoption

cmx keeps a tool-neutral canonical copy of hand-authored artifacts, separate
from any one assistant's install directory.

```bash
cmx home init                   # create the canonical home, register it as the `home` source
cmx home path [--json]          # print the resolved canonical home directory
cmx skill adopt <name> [<name>...] [--all] [--from-dir <dir>] [--local]
                                 # bring an orphaned, hand-authored skill under the home
cmx skill unadopt <name> [<name>...] [--external]
                                 # remove from the home and stop tracking it
                                 # (--external also marks it as managed by another tool)
```

### `cmx doctor` — the system-wide survey

`cmx doctor` is **read-only by default**: it walks every platform's install
directories and cross-references lock files, reporting artifacts that are
missing, untracked (orphaned), diverged across tools, or otherwise need
attention.

```bash
cmx doctor                      # global scope only, issues only
cmx doctor --local               # also survey project (local) scope
cmx doctor --all                 # show the full inventory, not just issues
cmx doctor --json                # machine-readable survey (structured; suppresses the table)

# To adopt orphans, use the canonical adopt commands — `cmx doctor --adopt-all`
# is deprecated (still works this release, removed next major):
cmx skill adopt --all            # canonicalize every orphaned skill into the home
cmx agent adopt --all            # ...and every orphaned agent
```

**Exit code contract:** `cmx doctor` exits `2` when it finds actionable
issues, `0` when the system is clean. Script against this — a non-zero,
non-2 exit means the command itself failed, not that it found something to
fix.

### Search and config

```bash
cmx search <query> [--json]              # keyword search across all sources
cmx config show [--json]                 # current configuration
cmx config gateway <openai|ollama>       # set the LLM gateway (llm-feature build)
cmx config model <name>                  # set the LLM model name
cmx config external {list,add,remove}    # rules for artifacts another tool manages
                                          # (doctor reports these as external, not orphaned)
cmx config platforms {list,add,remove}   # pin the managed platform set; when empty,
                                          # cmx infers it from platforms already in use
```

### `--json`

Every read-only data-reporting command emits `--json`: `list`, kind-scoped
`agent|skill list`, `outdated`, `search`, `info`, `source list`, `source
browse`, `set list`, `set show`, `config show`, `home path`, plus `doctor`
and `init`. Human-formatted output stays the default. JSON always goes to
stdout, empty results stay valid JSON (`[]` or an object with empty arrays),
and `cmx doctor --json` still exits `2` when it finds actionable issues.

### `cmx init` — cmx's own companion skill

cmx installs *this* skill through the same shared installer library
(`cmx-core`) that other fleet tools use, at **global scope by default**.

```bash
cmx init                 # install (or update) this skill into ~/.claude/skills/cmx/
cmx init --local          # install into .claude/skills/ in the current project instead
cmx init --force          # overwrite even if the installed copy is a newer version
cmx init --remove [--local]   # uninstall
cmx init --json           # machine-readable report instead of human text
```

`cmx init` refuses to downgrade an installed copy that's newer than the
bundled one unless `--force` is passed; that refusal exits non-zero.
`--global` is accepted as a no-op alias (global is already the default) so
scripts written before this alias existed keep working.

## Typical workflows

**Set up a new machine:**

```bash
cmx source add guidelines https://github.com/svetzal/guidelines
cmx skill install --all
cmx doctor
```

**Find and fix problems across every assistant on a machine:**

```bash
cmx doctor --local --all      # full inventory, both scopes
cmx skill adopt --all         # bring orphaned skills under management
cmx agent adopt --all         # ...and orphaned agents
```

**A skill drifted after an agent edited it in Cursor but not Claude:**

```bash
cmx skill diff <name>                 # see what changed (llm build)
cmx skill sync <name> --dry-run       # preview which copy would win
cmx skill promote <name> --from cursor       # or: make the Cursor edit canonical
```

**Stay current:**

```bash
cmx source update
cmx outdated
cmx skill update --all
```

## Tips

- Prefer `--all` for bulk install/update over enumerating names by hand.
- `cmx skill sync` is the reconciliation tool for divergence *between install
  locations*; `cmx skill update` is for pulling a newer version *from the
  source*. Use `diff`/`promote` to inspect and resolve, not raw file edits.
- `source:name` (e.g. `guidelines:code-review`) pins an install to one source
  when the same artifact name exists in more than one.
- Global scope is the default everywhere `--local` is offered; pass `--local`
  only when you specifically want a project-scoped install.
