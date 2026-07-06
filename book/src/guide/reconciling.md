# Reconciling Across Platforms

`cmx skill sync <name>` reconciles a skill that has **diverged across platforms**
— different content in different install locations — by copying one copy over the
others, so every platform carries the same thing.

Unlike `update` (which pulls from a registered *source*) and `promote` (which
pushes into the *home*), `sync` works **between install locations**. That means
it also reconciles `external` skills and any skill with no source at all — the
cases `update`/`promote` can't touch.

```bash
cmx skill sync personal-finance
```

`cmx doctor` flags divergence as `(diverged)` and its hint routes here when the
skill is external or source-less.

## Which copy wins

By default the **newest version wins**. Force the direction with `--from`,
preview with `--dry-run`, or reconcile project scope with `--local`:

```bash
cmx skill sync personal-finance --from codex      # codex's copy wins
cmx skill sync personal-finance --dry-run         # preview, write nothing
cmx skill sync personal-finance --local           # project scope
```

```text
$ cmx skill sync personal-finance
Reconciled 'personal-finance' from codex (v1.3.0):
  updated ~/.claude/skills/personal-finance (v1.2.0 → v1.3.0)  [claude]
```

## When it can't auto-pick

If the differing copies are **unversioned or share a version**, `sync` won't
guess. It lists each diverging copy (platforms, location, size) and prints the
exact per-copy `--from` command so you can choose — scoped to a managed platform,
so a copy shared by the `.agents/skills` cohort reads as `--from codex` rather
than `--from opencode`. When the skill is also tracked from the home, it points
at [`promote`](./promoting.md) as the make-one-copy-canonical-then-re-project
alternative.

## Skills only

`sync` is skills-only for now. Agents are reformatted per platform (e.g. Codex
TOML), so a byte-level cross-platform comparison — and a straight copy between
locations — isn't meaningful. Reconcile agents through their source (`update`) or
the home (`promote`) instead.

## sync vs promote vs update

- **`sync`** — reconcile a skill's copies **between install locations**; no
  source or home involved. Use this when you want every installed copy to match.
  Newest version wins by default.
- **[`promote`](./promoting.md)** — push the *installed* edits into the home,
  making them canonical.
- **`update`** — pull the *source* over one installed copy (default platform
  unless `--platform` is given).

After `sync`, re-run `cmx doctor` to confirm the divergence is gone. An exit `0`
means the command ran, not that some other untouched copy could not still need
attention.
