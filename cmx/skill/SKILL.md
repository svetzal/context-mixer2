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
  version: "0.1.0"
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

```
cmx source   {add,list,browse,update,remove}
cmx agent    {install,list,info,diff,update,sync,promote,uninstall,unadopt,adopt}
cmx skill    {install,list,info,diff,update,sync,promote,uninstall,unadopt,adopt}
cmx list     [--all]
cmx doctor   [--local] [--adopt-all] [--from <dir>] [--all]
cmx home     {init,path}
cmx outdated
cmx search   <query>
cmx info     <name>
cmx config   {show,gateway,model,external,platforms}
cmx init     [--local] [--force] [--remove] [--json]
```

`agent` and `skill` take the identical set of subcommands — pick the one that
matches what you're managing.

### Sources — where artifacts come from

```bash
cmx source add <name> <path-or-url>   # register a local path or git URL
cmx source list                       # show registered sources
cmx source browse <name>              # list agents/skills available in a source
cmx source update [<name>]            # git pull registered sources (default: all)
cmx source remove <name>              # unregister (does not delete installed artifacts)
```

### Install, list, and inspect artifacts

```bash
cmx skill install <name> [<name>...]     # install by name; `source:name` pins a source
cmx skill install --all                  # install everything available
cmx skill install <name> --local         # install into the current project instead of globally
cmx skill install <name> --force         # overwrite even if locally modified
cmx skill list [--all]                   # installed skills (--all includes externally-managed ones)
cmx skill info <name>                    # source, version, activation trigger; a generated
                                          # "what it does" summary too, in an `llm`-feature build
cmx skill uninstall <name> [<name>...] [--local]   # remove everywhere cmx tracks it
```

`cmx agent install/list/info/uninstall` work the same way for agents. There is
no separate plan/dry-run step for these — `install`/`uninstall` write
immediately. `cmx skill sync` is the one artifact subcommand with a
`--dry-run` preview (see below).

### Keeping things current

```bash
cmx outdated                    # installed artifacts with a newer source version
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
cmx skill promote <name> [--platform <tool>]
                                        # push in-place edits back into the canonical
                                        # home — the mirror of `update`; if several
                                        # platforms diverge, pick the winner with
                                        # --platform
```

### Canonical home and adoption

cmx keeps a tool-neutral canonical copy of hand-authored artifacts, separate
from any one assistant's install directory.

```bash
cmx home init                   # create the canonical home, register it as the `home` source
cmx home path                   # print the resolved canonical home directory
cmx skill adopt <name> [<name>...] [--all] [--from <dir>] [--local]
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
cmx doctor --adopt-all [--from <dir>]   # the one MUTATING doctor flag: adopt every
                                        # orphan into the canonical home
```

**Exit code contract:** `cmx doctor` exits `2` when it finds actionable
issues, `0` when the system is clean. Script against this — a non-zero,
non-2 exit means the command itself failed, not that it found something to
fix.

### Search and config

```bash
cmx search <query>                       # keyword search across all sources
cmx config show                          # current configuration
cmx config gateway <openai|ollama>       # set the LLM gateway (llm-feature build)
cmx config model <name>                  # set the LLM model name
cmx config external {list,add,remove}    # rules for artifacts another tool manages
                                          # (doctor reports these as external, not orphaned)
cmx config platforms {list,add,remove}   # pin the managed platform set; when empty,
                                          # cmx infers it from platforms already in use
```

### `--json`

As of this version, **only `cmx init` emits `--json`**. Every other command
prints human-formatted text; do not assume machine-readable output elsewhere.

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
cmx doctor --adopt-all        # bring orphans under management
```

**A skill drifted after an agent edited it in Cursor but not Claude:**
```bash
cmx skill diff <name>                 # see what changed (llm build)
cmx skill sync <name> --dry-run       # preview which copy would win
cmx skill promote <name> --platform cursor   # or: make the Cursor edit canonical
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
