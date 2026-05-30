# Canonical Home & `cmx doctor`

> **Status:** Living design document. Published in the open on purpose — it
> records *why* cmx grows a canonical home for hand-authored artifacts and a
> system-survey command, so users and contributors can see the design goals and
> trade-offs rather than reverse-engineering them. Last substantive update:
> 2026-05.

## Why this document exists

cmx already projects a curated set of artifacts *outward* into many tools'
native locations (see [Multi-Tool Platform Support](./multi-tool-platform-support.md)).
But it assumed the curated set already lived somewhere managed — a git-backed
marketplace or a tidy local source. Real systems don't start there. They start
as a pile of hand-authored skills and agents inside whatever tool you adopted
first (for many people, `~/.claude/`), with no provenance, no checksums, and no
way to move them to the next tool without copy-paste.

Two forces made this urgent:

1. **Tool portability is now a survival requirement, not a nicety.** A coding
   assistant's licensing or pricing can change such that you must move your
   automation to a different tool on a deadline. When that happens, your private
   skills must not be hostage to the tool you're leaving.
2. **The "curated set" has to have a home that outlives any one tool.** If the
   source of truth *is* `~/.claude/skills`, then dropping Claude Code means
   losing the source of truth. The home must be tool-neutral.

This note records the model we chose, the classification `cmx doctor` reports,
the adoption mechanics, and the scope boundaries.

## Design principles (how this extends the existing ones)

This work is a direct application of two commitments already stated in the
platform-support design doc, plus one addition:

- **One curated set, projected outward.** (existing) The canonical home *is* that
  curated set, made explicit and tool-neutral. Today's gap was that the set had
  no first-class home; you projected outward from an accident of history.
- **Provenance and integrity are first-class.** (existing) Adoption is the act of
  *giving* an un-provenanced artifact provenance: a checksum, a recorded version,
  a home. `doctor` is how you see provenance (or its absence) across the whole
  system.
- **Diagnose before you mutate.** (new) `doctor` is read-only by contract. You
  see the full truth of a disorganized system — what's tracked, drifted,
  orphaned, missing, duplicated — *before* any command changes a byte. Adoption
  is a separate, explicit step.

## The model

### Canonical home

A **first-class local directory** that holds your hand-authored private agents
and skills. It is the authoritative source of truth for artifacts you wrote
yourself (as opposed to artifacts you pulled from a remote marketplace).

- **Location:** `~/.config/context-mixer/home` by default — inside cmx's
  existing config root, alongside `sources.json` and the lockfiles — overridable
  via the `home` field in `config.json`. (We deliberately reuse the established
  config directory rather than invent a new `~/.cmx/` tree; cmx already owns
  `~/.config/context-mixer/`.)
- **Structure:** a plain artifact tree — `agents/*.md` and `skills/<name>/SKILL.md`
  — with **no `marketplace.json` required**. cmx's existing fallback
  tree-walking scanner already reads un-manifested repositories, so the home
  works with `list`, `install`, `search`, and `browse` unchanged. `cmf validate`
  / `cmf status` can lint and author it without new code.
- **First-class status:** the home is injected as an implicit, always-present
  source named `home`. It sorts first, cannot be removed via `source remove`,
  and is created on first use. Every existing command that iterates sources sees
  it for free — that is what makes it *first-class* rather than "just another
  `source add`."
- **`~/.claude/skills` is demoted.** It stops being special. It becomes one
  install *target* among the thirteen platforms, on equal footing with opencode,
  codex, and hermes. The source of truth now survives dropping any single tool —
  which is the entire point.

### `cmx doctor` — read-only stocktake

`doctor` surveys **every** platform's install directories (global, and `--local`
for project scope) and cross-references each per-platform lock file plus the
canonical home. For each artifact it reports one classification:

| State | Meaning | Typical cause |
|---|---|---|
| **tracked** | in a lock file, installed checksum matches the home | a normal cmx install |
| **drifted** | tracked, but the installed copy was edited after install | hand-tweaked in place |
| **orphaned** | present on disk, no lock entry, not in the home | hand-authored artifacts (the `~/.claude/skills` pile) |
| **missing** | in a lock file, but gone from disk | deleted out-of-band |
| **duplicated** | the same artifact lives in N platforms, possibly at different versions | manual copying between tools |

`doctor` mutates nothing. It exits non-zero when anything is off (so it is
usable in a hook or CI later) and prints a grouped, human-readable report plus a
short "what to do next" footer (e.g. "12 orphaned skills — run
`cmx doctor --adopt-all` to bring them under management").

### Adoption — orphaned → tracked

Adoption is the bridge from "disorganized pile" to "managed set":

- `cmx skill adopt <path>` (and `cmx agent adopt <path>`) copies one orphaned
  artifact into the canonical home, normalizes its frontmatter (fills `name`,
  defaults `version: 0.1.0` if absent), records its checksum, and registers it.
- `cmx doctor --adopt-all` sweeps every orphan `doctor` found into the home in
  one pass.
- **Adoption copies; it never moves the original.** After adoption the original
  on-disk copy reclassifies from *orphaned* to *tracked* (its checksum now
  matches the home). The destructive choice — deleting originals, or rewriting
  them as managed installs — is left to the user via existing `uninstall` /
  `install` commands. This keeps adoption safe to run on a messy system.

### Projection — already built

Once artifacts are in the home, projecting them to whatever tool you are moving
to needs **no new code**: the home is an authoritative source, so
`cmx skill install --all --platform opencode` (or `codex`, `hermes`, …) installs
the whole set into that tool's native location with full lockfile tracking.

The cutover ritual becomes:

```text
cmx doctor                 # see the mess
cmx doctor --adopt-all     # canonicalize the orphaned private artifacts
cmx skill install --all --platform <target>   # project to the new tool
```

## Architecture notes

The one non-trivial structural point: **`doctor` surveys all platforms, but the
existing path/lock machinery is bound to a single active platform** (`ConfigPaths`
carries one `Platform`, and `lock_path` / `install_dir` resolve against it). To
survey every platform without coupling, `doctor`:

- iterates `Platform::ALL` (a new exhaustive slice of variants), and
- for each platform derives a per-platform view of `ConfigPaths` (the `home_dir`
  and `config_dir` are platform-independent; only `platform` changes), then
  reuses the already-tested `installed_names`, `lock_path`, and lock-loading
  functions.

No platform-specific knowledge is duplicated into `doctor`; it composes the
existing pure functions per platform. This keeps the survey honest as new
platforms are added — a platform that appears in `Platform::ALL` is automatically
surveyed.

## Scope decisions

| Decision | Choice | Rationale |
|---|---|---|
| `doctor` mutation | **Read-only by contract** | Diagnose before you mutate. The only writing path is the explicit `adopt` / `--adopt-all` flag. |
| Adoption of originals | **Copy, never move** | Safe to run on a messy system; reclassifies originals to *tracked* without risking data loss. Deleting/migrating originals stays an explicit, separate user action. |
| Home structure | **Plain tree, no manifest** | The fallback scanner already reads un-manifested repos; requiring `marketplace.json` for purely-local private artifacts would be friction with no benefit. |
| Home location | **`~/.config/context-mixer/home`, config-overridable** | Reuses cmx's existing config root (next to `sources.json` and the lockfiles) rather than inventing a new `~/.cmx/` tree. Still tool-neutral — it lives under cmx's own directory, not any single assistant's — so it outlives tool changes. |
| Home as a source | **Implicit, always-present, unremovable** | This is what makes it first-class; every source-iterating command sees it for free, and it can't be accidentally `source remove`d. |
| `~/.claude/skills` | **Demoted to an install target** | Decouples the source of truth from the tool being abandoned — the motivating requirement. |

## Phasing

- **Phase 1 — `doctor` (read-only):** the cross-platform survey and
  classification. Ship first so the system state is visible before any model
  decision about the home is finalized.
- **Phase 2 — canonical home + `adopt`:** the implicit `home` source, the
  `config.json` `home` field, `cmx {skill,agent} adopt`, and
  `cmx doctor --adopt-all`.
- **Phase 3 — projection ergonomics (optional):** a `cmx sync` convenience that
  fans the home out to a configured set of platforms in one command. Not needed
  for the cutover — `install --all --platform X` already covers it — so this is a
  fast-follow only if the per-platform invocation proves tedious.

## Source code map (planned)

- `cmx/src/platform.rs` — add `Platform::ALL` (exhaustive variant slice) so the
  survey is automatically complete.
- `cmx/src/paths.rs` — add a per-platform `ConfigPaths` view (done in Phase 1)
  and `artifact_home_dir()` resolving the canonical home under `config_dir`
  (`config_dir.join("home")` by default — *not* the existing `home_dir` field,
  which is the OS home).
- `cmx/src/types.rs` — add the `home` field to `CmxConfig`; add the artifact
  classification enum used by `doctor`.
- `cmx/src/doctor.rs` (new) — the read-only cross-platform survey + classification
  (pure functional core over injected `Filesystem`).
- `cmx/src/adopt.rs` (new) — copy-into-home + frontmatter normalization +
  lockfile/registration.
- `cmx/src/source_iter.rs` — inject the implicit `home` source into iteration.
- `book/src/reference/commands.md` — document `doctor` and `adopt`.
