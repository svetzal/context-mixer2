# EMBEDDING.md — the cmx-core library extraction

**Status: cmx-core 0.1.0 PUBLISHED to crates.io 2026-07-03** — after two proving consumers (parite single-file, foundry multi-file) and an API-stabilization pass driven by their friction reports. Original phase-1 note follows.

**PHASE 1 SHIPPED 2026-07-03** — `cmx-core` crate extracted (`3e1924d`) and the embeddable `SkillInstaller` API added (`1004cdb`); decisions below are implemented. One deliberate deviation from the original sketch: `install`/`copy`/`adopt` stayed in the cmx crate, because they transitively depend on `scan → scan_marketplace → plugin_types` (excluded from core by decision 5) — cmx-core carries the clean lower layer (types, platform, paths, gateways, lockfile, checksum, config) plus its own self-contained `skill_fs`/`skill_install` path. Next: migrate parite's `init` to cmx-core, then spec + conformance fixtures, then Python/TypeScript ports.

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
// One-call context for Claude Code tools (use from_env(Platform::X) for others)
let prod_ctx = ProductionContext::claude()?;
let ctx = prod_ctx.ctx();

let skill = BundledSkill::single_md(include_str!("../skills/parite/SKILL.md"));
let installer = SkillInstaller::new(ToolIdentity::new("parite", "1.4.0"));
let plan = installer.plan(&skill, Scope::Global, false, &ctx)?; // dry-run: renderable, precise
println!("{plan}");
let report = installer.apply(&skill, &plan, &ctx)?;             // copy + lock entry
println!("{report}");
installer.status(Scope::Global, &ctx)?;                         // installed? version? drifted?
installer.remove(Scope::Global, &ctx)?;                         // uninstall + lock cleanup
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

## Decisions (reviewed with Stacey, 2026-07-03)

1. **Naming: `cmx-core`** — everywhere (crates.io, PyPI, npm). The lockfile format is cmx's; honesty about that beats false neutrality.
2. **Lock entries without cmx present: yes** — the lockfile format is the integration contract. Tools write entries into `~/.config/context-mixer/` even on machines that have never seen cmx, so a later cmx arrival finds everything tracked.
3. **Command convention: `<tool> init`** — the standard companion-skill command across the fleet. gilt's `skill-init` folds into `init` during migration; codified in `conventions/cli-ux.md` §12.
4. **Scope default: global** — skills install to the user's global platform directory (`~/.claude/skills/`, etc.) by default; `--local` opts into project scope. A tool's companion skill describes the tool, not the project.
5. **cmf stays external for now** — the marketplace/manifest machinery (`plugin_types`) remains CLI-side; cmx-core carries only what embedding tools need.
