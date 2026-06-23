# Promoting Local Edits

`cmx {skill,agent} promote <name>` pushes in-place edits of an installed artifact
back into your [canonical home](./under-control.md) — it is the **mirror of
`update`**.

- `update` pulls the *source* copy over your installed one, discarding any local
  edits.
- `promote` copies your *installed* copy into the home and refreshes its
  `home`-provenance lock baselines, so the artifact reads as `tracked` again with
  your edits as the new canonical content.

## The authoring loop

This supports the common way skills actually evolve: an assistant (or you) edits
a skill **where it's installed**, then you promote those edits to the home so they
become the source of truth — and project out from there.

```bash
# … an assistant improves ~/.agents/skills/personal-finance in place …

cmx skill diff personal-finance      # see what changed, and in which direction
cmx skill promote personal-finance   # make those edits canonical in the home
```

`promote` operates on the copy `cmx diff` shows — global scope preferred, then
project. After promoting, the home holds your edits and any other platform still
carrying the old content is reported as drifted and pointed at
[`sync`](./reconciling.md).

## Example

```text
$ cmx skill promote personal-finance
Promoted 'personal-finance' (1.3.0) into the home: ~/.config/context-mixer/home/skills/personal-finance
  re-tracked for: claude, codex
```

## When promote is refused

The target is the **home** only, so `promote` declines cases where the installed
copy isn't a canonical home candidate, and steers you to the right command
instead:

| Situation | Why | Do this instead |
|-----------|-----|-----------------|
| Artifact is tracked from a **git source** | the source, not the home, is authoritative | edit the source clone and `cmx <kind> update`, or `update --force` to discard the edits |
| Artifact is **untracked** / orphaned | nothing tracks it yet | `cmx <kind> adopt <name>` (hand-authored) or `cmx <kind> install <name>` (source-backed) |
| A **Codex agent** | its installed copy is TOML, not canonical markdown | edit the markdown in the home or source directly |

## promote vs sync vs update

- **`promote`** — keep the *installed* edits, making them canonical in the home.
- **[`sync`](./reconciling.md)** — reconcile a skill's copies **between install
  locations** (no source involved); newest version wins by default.
- **`update`** — pull the *source* over the installed copy (discards local edits;
  `--force` to overwrite a modified one).

`cmx skill diff` recommends a direction and prints the exact `promote` /
`update` commands in its reconcile footer.
