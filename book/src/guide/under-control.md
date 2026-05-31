# Bringing an Existing System Under Control

If you've used AI coding assistants for a while, your agents and skills are
probably scattered and unmanaged: hand-authored skills in `~/.claude/skills`, a
tool's bundled defaults in `~/.hermes/skills`, things you installed once and
edited, lock entries pointing at sources that moved. This guide walks the
real-world process of bringing that pile under cmx control, using the situations
you'll actually hit.

The whole process is driven by one read-only command:

```bash
cmx doctor
```

`doctor` surveys every supported platform's install directories and lock files
and classifies each artifact. It changes nothing — it just tells you the truth.
A typical first run on a lived-in system:

```text
Summary: 18 tracked, 3 drifted, 0 untracked, 43 orphaned, 0 external, 1 missing · 0 diverged.
```

Work through it one state at a time. Each section below is a state `doctor`
reports, what it means, and how to resolve it.

## `missing` — a lock entry with no file

A lock file records the artifact, but it's gone from disk. Usually it was
deleted out-of-band (not via `cmx uninstall`, which would have cleared the lock
entry too).

First decide whether it's recoverable. If a registered source still provides it,
reinstall:

```bash
cmx skill install <name>
```

If the source no longer has it either — it was genuinely retired — the lock
entry is just cruft. Clear it:

```bash
cmx skill uninstall <name>
```

`uninstall` reconciles a missing entry even though the file is already gone: it
removes the stale lock entry and tells you the file was already absent. (Check
the source *before* concluding it's gone — see the drift section below, where a
"vanished" skill turned out to have just moved.)

## `drifted` — tracked, but edited since install

The installed copy's checksum no longer matches what was recorded at install.
Three things masquerade as drift; diagnose before you act.

### 1. Transient build artifacts (not real drift)

A skill that ships runnable scripts will look drifted the moment you run them —
`npm install` creates `node_modules/`, Python creates `__pycache__/`. cmx
**ignores** these (and `*.pyc`, `.git/`, `.DS_Store`) when checksumming, so they
shouldn't cause drift. If a script-bearing skill is still drifted after an
upgrade, the change is in real content, not the artifacts.

### 2. The source moved or advanced, your copy didn't change

Compare your installed copy against the *current* source. If they're identical
(ignoring `node_modules`), your copy has no real edits — the drift is just a
stale install-time snapshot. Re-sync:

```bash
cmx skill update <name> --force
```

This is safe: there are no local edits to lose.

> **Watch for relocations.** A source repo restructure can *move* a skill (e.g.
> from `skills/x` to `plugins/foo/skills/x`) without removing it. `cmx search
> <name>` finds it by name regardless of path — so confirm a skill is truly gone
> before treating it as unrecoverable. cmx tracks by name, so `update` still
> works across a relocation.

### 3. Your copy is genuinely ahead of the source

Sometimes you improved the installed copy and the work never went back upstream
— extra files, a sharper prompt. `diff` your copy against the current source to
see exactly what you added. You have two good options:

- **Push it upstream** (preferred when the source is yours or accepts
  contributions): copy your version into the source repo, commit and push, then
  `cmx skill update <name> --force` re-syncs cleanly and the drift disappears —
  with your work now shared.
- **Keep it local**: leave it as-is for now, or canonicalize it (see *orphaned*
  below — though adopt currently targets orphans, not drifted artifacts).

Either way, **never `update --force` a genuinely-ahead copy without capturing
your changes first** — force overwrites them with the source version.

## `untracked` vs `orphaned` — the distinction that keeps adoption safe

Both mean "on disk, no lock entry." The difference is whether a **registered
source provides it**, and it determines the right fix:

- **`untracked`** — a source *does* provide it; you installed it out-of-band.
  The fix is to **track** it, recording provenance:

  ```bash
  cmx skill install <name>
  ```

- **`orphaned`** — **no** source provides it; it's hand-authored (or from a
  source you never registered). This is the **adopt** candidate — see below.

This split is why adoption is safe: `adopt` and `--adopt-all` act **only on
orphaned** artifacts. Naming an untracked artifact to `adopt` steers you to
`install` instead, so you never adopt source-backed content as if it were your
private work.

## `orphaned` — your hand-authored artifacts → adopt into the home

Orphaned artifacts have no source, so they belong in your **canonical home**
(`~/.config/context-mixer/home` by default) — the tool-neutral source of truth
that survives switching assistants. Adopting copies the artifact verbatim into
the home, registers the home as a source, and records provenance so it
reclassifies to `tracked`. The original is never moved.

### Not everything orphaned is yours

The trap: a tool's **bundled/stock skills** also show up as orphaned (they
shipped with the tool, untracked). Adopting *those* into your home pollutes it
with a vendor's defaults. So a blanket `cmx doctor --adopt-all` is usually too
blunt — it adopts every orphan, including the stock bundle.

Curate instead. List the orphans and their locations:

```bash
cmx doctor          # the Location column tells you where each lives
```

Then adopt selectively. By location (everything you authored lives in one
place):

```bash
cmx skill adopt --all --from ~/.claude/skills
```

…which adopts your skills under that directory while leaving, say,
`~/.hermes/skills` (a tool's stock bundle) untouched. Or by name, for a hand-
picked set:

```bash
cmx skill adopt clipboard gilt foundry whatsapp
```

Named adoption is all-or-nothing — if any name isn't an adoptable orphan, the
batch aborts with the reason, and nothing is copied.

## `external` — artifacts another tool manages

Some orphans aren't yours and never will be: a tool's bundled/stock skills,
sitting in its own directory. You don't want to adopt them, but you also don't
want `doctor` nagging about them forever. Declare them **external** — cmx then
reports them as `external` (a steady state, not flagged) and adoption skips them
entirely. (The one thing `doctor` will still surface is a *divergence* — an
external artifact whose copies disagree across locations — since that's a real
anomaly, even though its owning tool, not cmx, is the one to re-sync it.)

```bash
cmx config external add ~/.hermes/skills    # the whole stock directory
cmx config external add some-vendored-skill # or a single artifact by name
cmx config external list                    # review the rules
```

A directory rule (with `~` expanding to your home) covers everything under it,
including skills the tool adds later; a bare name matches that artifact
anywhere. External artifacts still appear in `doctor` for visibility — they just
no longer count toward the non-zero exit, and `cmx <kind> adopt` refuses them
(pointing you to remove the rule first if you ever do want to manage one).

This is the difference between *orphaned* (yours, hand-authored → adopt) and
*external* (another tool's → leave it): both have no cmx lock entry, but only
the first is your responsibility.

## Projecting outward (the payoff)

Once your artifacts are in the home, it's a normal registered source. Migrating
your curated set onto another tool is one command per target:

```bash
cmx skill install --all --platform opencode    # or codex, pi, hermes, …
```

This is the point of the whole exercise: you curate once, in a tool-neutral
home, and project into each assistant's native location and format — so your
library outlives any single tool.

## Converging

Re-run `cmx doctor` after each step. `doctor` exits non-zero while any drift,
untracked, orphaned, or missing artifact remains (`external` and `tracked` don't
count), so you can drive it to a *clean* resting point — and even wire it into a
hook or CI check once you're there. A fully-curated system looks like:

```text
Summary: 40 tracked, 0 drifted, 0 untracked, 0 orphaned, 24 external, 0 missing · 0 diverged.
```

…where the `external` count is a tool's stock bundle you've deliberately marked
(see above), so `doctor` exits zero — everything that's *yours* is tracked, and
everything that isn't is acknowledged but unflagged.

> **One skill, many tools.** A skill you've projected to several assistants is
> reported as a *single* tracked artifact whose `Tools` column lists every tool
> it's installed for — not as "duplicates." `diverged` is reserved for the rare
> case where copies actually disagree (different version or state across
> locations); fix those with `cmx <kind> update <name> --force` to re-sync every
> copy from one source.
