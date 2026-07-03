# EMBEDDING.md — the cmx-core library extraction

**Status: DRAFT for review — no code has been extracted yet.**

This document imagines the path from cmx-the-CLI to cmx-the-library: a reusable core that our other CLI tools embed to install their companion agent skills, instead of each hand-rolling its own mechanism.

## The problem

Our CLI tools ship companion agent skills, and every one of them has invented its own installation machinery (surveyed 2026-07-03):

| Tool | Command | Embed method | Version guard | cmx-aware | Uninstall |
| ---- | ------- | ------------ | ------------- | --------- | --------- |
| parite (Rust) | `parite init` | `include_str!` | hand-rolled semver | no | none |
| gilt (Python) | `gilt skill-init` | package paths | hand-rolled semver | no | none |
| hopper (Bun/TS) | `hopper init` | Bun text import | hand-rolled semver | no | none |
| hone (Bun/TS) | `hone init` | Bun text import | hand-rolled semver | no | none |
| researcher (Python) | `researcher init` | importlib.resources | hand-rolled semver | no | none |
| evt (Python) | *(no skill yet)* | — | — | — | — |

Five independent implementations of the same idea: parse frontmatter version, compare semver, copy files into a hard-wired `.claude/skills/<name>/` under either `$HOME` or the project root. Each is subtly different (`skill-init` vs `init`; different frontmatter version keys: `hopper-version:`, `hone-version:`, `researcher-version:`, `metadata.version`). None can uninstall. None knows about any platform other than Claude. And none integrates with cmx — on a cmx-managed system, `cmx doctor` sees every one of these skills as an untracked orphan.

## The vision

A tool bundles its skill and makes one library call:

```text
"Here is my skill (name, version, files). Install it at this scope.
 Tell me what you did."
```

The library:

1. **Detects cmx management.** If the machine or project is cmx-managed (config/lockfiles present), it registers the bundled skill as a source and records a proper lock entry — the skill becomes a first-class tracked artifact that `cmx doctor`, `cmx update`, and `cmx list` all understand.
2. **Falls back gracefully.** With no cmx present, it performs the platform-aware, version-guarded copy the tools each hand-roll today — but consistently, and it *still writes the lock entry*. The lockfile format, not the cmx binary, is the integration contract: a later `cmx` arrival finds everything already tracked instead of orphaned.
3. **Plans before applying.** Consistent with our CLI UX conventions (guidelines repo, `conventions/cli-ux.md`): the install can be rendered as a dry-run plan, names each file and destination, and reports what changed in countable terms.
4. **Uninstalls.** Tools finally get `<tool> init --remove` (or equivalent) for free, honoring "leave the machine as you found it."

This preserves the tools-stay-independent rule (Operations `AGENTS.md`): tools depend on a *library* and share state through a *schema'd lockfile* — a neutral artifact. No tool shells out to the `cmx` CLI; cmx could be deleted and every tool still installs its skill correctly.

## What exists already

cmx is well-positioned for this (architecture reviewed 2026-07-03):

- `cmx/src/lib.rs` already exports the needed modules publicly: `types` (Artifact, LockFile, LockEntry, InstallScope), `platform` (14 platforms with per-platform install paths), `paths` (ConfigPaths), `install`, `lockfile`, `copy`, `checksum`, `adopt`.
- I/O is already behind gateway traits (`gateway::Filesystem`, `Clock`, `GitClient`) bundled in `AppContext` — the testability seam an embeddable library needs.
- The lockfile format (`cmx-lock.json` / `cmx-lock-<platform>.json`) already carries provenance (source repo + path) and dual checksums (source vs installed) for drift detection.

Gaps the extraction must close:

- **No high-level orchestrator** for "register bundled source → install to managed platforms → record lock entry." `install::install()` assumes a pre-registered source.
- **No production `AppContext` factory** — only tests construct one conveniently.
- **No public "is this platform managed here?" query** — the logic is internal to `resolve_targets()`.
- **No uninstall path** at all.

## Proposed shape

### The API surface (deliberately small)

```rust
let installer = SkillInstaller::new(ToolIdentity { name: "parite", version: "1.4.0" });
let plan = installer.plan(bundled_skill, Scope::Global)?;   // dry-run: renderable, precise
let report = installer.apply(plan)?;                         // copy + lock entry
installer.status(...)?;                                      // installed? version? drifted?
installer.remove(...)?;                                      // uninstall + lock cleanup
```

Plan/apply as first-class API mirrors the `--apply` convention, so tools can expose `mytool init --dry-run` trivially. The version-guard semantics (older→update, same→skip, newer→refuse unless forced) are standardized once, matching what all five tools independently converged on.

### Packaging across ecosystems

Targets align with the mojentic framework's ports: **Rust, Python, TypeScript, Elixir, Swift, Kotlin**. Proposed approach:

1. **`cmx-core` Rust crate** (crates.io) — extracted from cmx within this workspace; the reference implementation. The `cmx` binary becomes its first consumer.
2. **A compact behavioral spec** — lockfile schema, path-resolution rules, version-guard semantics, cmx-detection rules — plus shared conformance fixtures (golden lockfiles, before/after directory trees). This is what makes ports *ports* rather than divergent cousins, the same discipline as mojentic's PARITY.md.
3. **Native ports, demand-driven** — Python (`cmx-core` on PyPI: gilt, researcher, evt) and TypeScript (npm: hopper, hone) are needed immediately. Elixir/Swift/Kotlin follow when a tool in that ecosystem ships a skill, keeping parity with mojentic's target list without building ahead of need.

Native ports over FFI bindings: the domain is file copying, JSON lockfiles, and semver comparison — small enough that a port is cheaper than dragging a Rust toolchain into gilt's pure-Python build or complicating hopper's `bun build --compile` single-binary story. The conformance suite carries the correctness burden.

### Migration path

1. **Extract** `cmx-core` as a workspace crate; move `types`, `paths`, `platform`, `install`, `lockfile`, `copy`, `checksum`, `gateway` into it; add the orchestrator API, context factory, managed-platform query, and uninstall. `cmx` CLI consumes it.
2. **Prove it in-ecosystem**: migrate parite's `init` (Rust) to `cmx-core`. This validates the embeddable API against a real consumer before any porting begins.
3. **Write the spec + conformance fixtures** from the now-stable Rust behavior.
4. **Port** to Python and TypeScript; migrate gilt, researcher, hopper, hone. Standardize the command (`<tool> init`; gilt's `skill-init` folds into this) and the frontmatter version key (`metadata.version`, per the guidelines repo's baseline standards).
5. **Close the gaps**: evt gains a skill; foundry's registry `installs_skill` keeps working unchanged (it just invokes each tool's `init`, which now runs through cmx-core underneath).

## Open questions (for review before any extraction starts)

1. **Naming** — `cmx-core` everywhere (crate/PyPI/npm), or a standalone name that doesn't presume cmx (e.g. `skillbase`)? Leaning `cmx-core`: the lockfile format is cmx's, and honesty about that beats false neutrality.
2. **Lock entries without cmx present** — recommended above (the format is the contract), but it means tools write into `~/.config/context-mixer/` on machines that have never seen cmx. Comfortable with that footprint?
3. **Command convention** — settle `<tool> init` as the standard companion-skill command in `conventions/cli-ux.md` once this ships? (`init` currently also does non-skill setup in some tools.)
4. **Scope default** — tools today default to local (project) scope; cmx thinking is global-first. Which default serves the "up-arrow, add --apply" workflow better?
5. **cmf's role** — does the marketplace/manifest side (`plugin_types`) belong in cmx-core, or stay CLI-side? Initial instinct: stay CLI-side; tools don't need it.
