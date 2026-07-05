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
compatibility: Rust binary `cmx`. All commands work in every build; an `llm`-feature build with a configured gateway additionally generates `info` summaries and `diff` analyses (both degrade gracefully without one).
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

## Agent contract 1 — read state with `--json`, not tables

Every data-reporting command accepts `--json`. When *you* (an agent) are
reading cmx state to reason about it, always pass `--json` and parse with
`jq`. The human tables are lossy on purpose (truncated descriptions,
presentation words, aligned columns); the JSON is the source of truth and
carries full descriptions, `null` for absent values, and stable enum strings.
Use the bare human form only when displaying output to the user verbatim.

Shapes you can rely on (JSON to stdout; empty results are valid JSON):

```text
cmx list --json / cmx skill|agent list --json / cmx outdated --json
  {"artifacts": [{"name", "kind": "skill"|"agent", "scope": "global"|"local",
                  "source", "platforms": [..],
                  "installed_version": str|null, "available_version": str|null,
                  "status": "ok"|"unversioned"|"outdated"|..,
                  "locally_modified": bool (outdated only)}]}
  (version null = no version metadata in frontmatter, not "not installed")

cmx search <q> --json   → results with full untruncated "description"
cmx info <name> --json  → flat object: path, version, installed_at, source,
                          installed_checksum/source_checksum/disk_checksum,
                          locally_modified, activation_description,
                          summary (str|null — null when no LLM available)
cmx doctor --json       → {"artifacts": [{name, kind, scope, state, diverged,
                            locations: [{path, state, version}], ..}],
                           "summary": {tracked, drifted, untracked, orphaned,
                                       external, missing, diverged,
                                       set_inconsistent},
                           "showing": "needs_attention"|"all",
                           "scope", "platforms_surveyed", "set_inconsistencies"}
cmx config show --json  → {gateway, model, external: [..], platforms: [..],
                           platforms_inferred}
cmx home path --json    → {"path": ..}
cmx set list --json     → {"scope", "sets": [..]};  set show --json analogous
```

Idioms:

```bash
cmx outdated --json | jq -r '.artifacts[].name'                  # what needs updating
cmx doctor --json | jq -r '.artifacts[] | select(.diverged) | .name'   # cross-platform drift
cmx doctor --json | jq -e '.summary.orphaned == 0' >/dev/null    # assert clean adoption
cmx list --json | jq -e '.artifacts[] | select(.name=="foo")'    # is foo installed?
cmx info foo --json | jq -r '.locally_modified'                  # safe to update?
```

**Exit codes are API:** `cmx doctor` exits `2` when it finds actionable
issues, `0` when clean (both human and `--json` forms). Any other non-zero
exit means the command itself failed.

## Agent contract 2 — mutations plan by default

The reconciliation commands — `skill|agent sync`, `skill|agent promote`,
`set activate`, `set deactivate` (and `set delete --purge`) — are
**plan/apply**: run bare, they print a concrete plan (exact files, paths,
versions, per-file +/− counts), write **nothing**, and exit 0. Re-run the
identical command with `--apply` to execute exactly that plan. Read the plan
before applying. (`--dry-run` survives one release as a deprecated alias for
the default plan mode and warns on stderr.)

`install`, `uninstall`, and `update` execute immediately (package-manager
convention) — but when `--force` overwrites local edits on install/update,
cmx first prints the exact file paths whose changes are being discarded.
`--force` always means "override a safety refusal", never "skip confirmation".

Help text marks the boundary: `[Mutates]` = writes immediately,
`[Mutates with --apply]` = plans by default. Unmarked commands are read-only.

Errors teach the next step — argument mistakes print a `try: <exact command>`
line, unknown artifact names suggest near-matches (`Did you mean 'cli-ux'?`),
and LLM/gateway failures degrade to a one-line note (never a hard failure of
the surrounding command). Trust stderr guidance; don't guess flags.

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
cmx completions <bash|zsh|fish|elvish|powershell>
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
cmx set activate <name>   [--apply]      # plan by default; --apply executes
cmx set deactivate <name> [--apply] [--force]
                                          # uninstall every member not held by another
                                          # active set = "turn this set off"; remembers
                                          # membership so re-activating reinstalls fresh
cmx set delete <name> [--purge] [--apply]  # --purge plans the deactivate+delete; --apply executes
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
cmx skill install <name> --force         # overwrite even if locally modified (lists discarded files)
cmx skill list [--all] [--json]          # installed skills (--all includes externally-managed ones)
cmx skill info <name> [--json]           # source, version, activation trigger; a generated
                                          # "what it does" summary too, in an `llm`-feature build
cmx skill uninstall <name> [<name>...] [--local]   # remove everywhere cmx tracks it
```

`cmx agent install/list/info/uninstall` work the same way for agents.

### Keeping things current

```bash
cmx outdated --json | jq -r '.artifacts[].name'   # what has a newer source version
cmx skill update <name>         # pull the latest version from its source
cmx skill update --all          # update every tracked skill
cmx skill update <name> --force # overwrite even if locally modified (lists discarded files)
```

Check `cmx info <name> --json | jq .locally_modified` before updating a
single artifact — a `true` means plain `update` will refuse and you must
decide between `--force` (discard edits) and `promote` (keep them; below).

### Reconciling drift across tools and canonical homes

Skills installed in more than one assistant's directory can diverge (an agent
edits the Claude copy but not the Cursor copy). Three commands handle this:

```bash
cmx skill diff <name>                  # structural diff + change summary, always available;
                                        # an `llm`-feature build adds an LLM-written analysis
cmx skill diff <name> --full           # full line-by-line unified diff (never needs an LLM)
cmx skill sync <name> [--from <platform>] [--apply] [--local]
                                        # plan: copy one platform's copy over the others
                                        # (works for "external" skills too); --apply executes
cmx skill promote <name> [--from <platform>] [--apply]
                                        # plan: push in-place edits back into the canonical
                                        # home (the mirror of update); --apply executes
```

Decision rule (doctor prints the same guidance): edited in place and want to
keep it → `promote`; want the source version back → `update --force`;
external/source-less divergence → `sync`; unsure → `diff` first.

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

`cmx doctor` is **read-only**: it walks every platform's install directories
and cross-references lock files, reporting artifacts that are missing,
untracked (orphaned), drifted, diverged across tools, or external.

```bash
cmx doctor --json                # machine-readable survey — the agent default
cmx doctor                       # human table, issues only
cmx doctor --local               # also survey project (local) scope
cmx doctor --all                 # full inventory, not just issues

# To adopt orphans, use the canonical adopt commands — `cmx doctor --adopt-all`
# is deprecated (still works this release, removed next major):
cmx skill adopt --all            # canonicalize every orphaned skill into the home
cmx agent adopt --all            # ...and every orphaned agent
```

**Exit code contract:** `2` = actionable issues found, `0` = clean, anything
else = the command itself failed. Script against this.

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

### Shell completions

```bash
cmx completions zsh                       # completion script to stdout (also: bash, fish,
                                          # elvish, powershell); pipe/redirect to install
```

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

`cmx init` protects existing copies with two safety refusals, both exiting
non-zero and both overridden by `--force`: it won't downgrade an installed
copy newer than the bundled one, and it won't overwrite a copy with local
edits (detected against disk, same as `info`/`diff`) — the skip hints
`--force` to overwrite or `cmx skill promote cmx` to keep the edits. When
`--force` discards edits it prints the exact file paths first. `--global`
is accepted as a no-op alias (global is already the default) so scripts
written before this alias existed keep working.

## Typical workflows (agent-shaped)

**Set up a new machine:**

```bash
cmx source add guidelines https://github.com/svetzal/guidelines
cmx skill install --all
cmx doctor --json | jq .summary        # expect zeros; exit 2 = something to fix
```

**Survey and fix a messy multi-tool setup:**

```bash
cmx doctor --local --all --json > /tmp/survey.json
jq -r '.artifacts[] | select(.state=="orphaned") | .name' /tmp/survey.json
cmx skill adopt --all && cmx agent adopt --all
jq -r '.artifacts[] | select(.diverged) | .name' /tmp/survey.json
# then per name: diff → promote / update --force / sync (decision rule above)
```

**A skill drifted after an agent edited it in Cursor but not Claude:**

```bash
cmx skill diff <name>                        # inspect (works without an LLM)
cmx skill promote <name> --from cursor       # plan: make the Cursor edit canonical
cmx skill promote <name> --from cursor --apply   # execute that exact plan
```

**Stay current:**

```bash
cmx source update
cmx outdated --json | jq -r '.artifacts[] | select(.locally_modified | not) | .name' \
  | xargs -n1 cmx skill update
```

## Tips

- Prefer `--all` for bulk install/update over enumerating names by hand.
- `sync` reconciles *between install locations*; `update` pulls *from the
  source*; `promote` pushes in-place edits *back to the canonical home*. Use
  `diff` to inspect — never resolve drift with raw file edits.
- `source:name` (e.g. `guidelines:code-review`) pins an install to one source
  when the same artifact name exists in more than one.
- Global scope is the default everywhere `--local` is offered; pass `--local`
  only when you specifically want a project-scoped install.
- Plans are cheap and read-only — when unsure what a reconcile command will
  do, run it bare and read the plan; nothing happens until `--apply`.
