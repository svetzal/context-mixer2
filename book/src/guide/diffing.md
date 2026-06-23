# Diffing with LLM Analysis

`cmx diff` shows how an installed artifact differs from its source — directionally,
so you can see which copy holds which change and pick the right way to reconcile.

```bash
cmx agent diff python-craftsperson
cmx skill diff personal-finance
```

If every installed copy is byte-identical to the source, cmx reports "matches"
without calling the LLM. (The `diff` command requires a build with the **`llm`
feature**; a lean build doesn't include it.)

## What you get

The output names **both sides with their real names** — the source (`home`, or
the repo) and the platform whose copy is shown (e.g. `codex`) — and uses them
consistently, so you never have to map "installed"/"source" onto an actual copy.
It stays **compact** by default:

- a header naming each side, its path and version, and flagging the installed
  copy as **edited locally** when its bytes no longer match the lock baseline;
- a **per-file change summary** — `M` modified (with `+added −removed` counts),
  `A` added (only in the installed copy), `D` deleted (only in the source) —
  under a stated convention: `−` lines are the source, `+` lines are the
  installed copy;
- an **LLM summary** of what changed and which direction it recommends;
- a **reconcile footer** offering *both* directions and picking neither.

The full line-by-line unified diff prints only with `--full` (a one-line hint
points there), so a large change reads in ~20 lines instead of hundreds:

```bash
cmx skill diff personal-finance --full
```

## Example output

```text
Comparing personal-finance (skill) vs home

  codex   ~/.agents/skills/personal-finance                       (1.3.0, edited locally)
  home    ~/.config/context-mixer/home/skills/personal-finance    (1.3.0)

− lines are home, + lines are codex:

Changed files:
  M  SKILL.md                                    +12  −3
  A  helpers/categorize.py                       only in codex

Summary:
The installed copy adds a categorisation helper and tightens the SKILL.md
trigger wording. Behavioural addition, not a fix — keep it if the edits are
yours.

Reconcile — pick a direction:
  keep codex's edits — copy codex into home          cmx skill promote personal-finance
  discard codex's edits — pull home over codex       cmx skill update personal-finance --force
      (overwrites your local edits)

(run with --full to see the line-by-line diff)
```

The two reconcile directions map to two commands: **`promote`** keeps the
installed edits by copying them into the home (offered when the source is the
home — see [Promoting Local Edits](./promoting.md)), and **`update --force`**
discards them by pulling the source over the installed copy. `diff` recommends a
direction but commits to neither.

## Multi-platform aware

`diff` agrees with [`cmx doctor`](./under-control.md): it surveys **every**
installed copy of a skill across your managed platforms, not just the active
one. When more than one copy exists it shows a per-platform matrix — which copies
match the source, which differ — and focuses the detailed diff on a copy that
actually differs, so it never reports "matches home" while another platform's
copy has drifted:

```text
  claude   ~/.claude/skills/personal-finance     matches home
  codex    ~/.agents/skills/personal-finance     differs from home (+12 −3)   ← detailed below
```

The reconcile commands are qualified with `--platform` for the focused copy
(preferring a managed platform, e.g. `--platform codex` over `opencode`), and a
fully-consistent skill reports "matches home on all N installed copies". Agents
stay single-copy — they're reformatted per platform (e.g. Codex TOML), so a
byte-level cross-platform comparison isn't meaningful.

## Works with untracked artifacts

`diff` works on any installed artifact that has a matching source — even ones not
tracked in the lock file. It finds the file on disk and compares directly.

## LLM configuration

See [Configuration](./configuration.md) to set up the LLM gateway and model.
