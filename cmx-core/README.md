# cmx-core

Embeddable core for installing agent skills across platforms. Extracted from [cmx](https://github.com/svetzal/context-mixer2), the context-mixer CLI.

CLI tools that ship a companion agent skill use this crate to install it, instead of hand-rolling file copies into hard-wired paths:

```rust
use cmx_core::production::ProductionContext;
use cmx_core::skill_install::{BundledSkill, Scope, SkillInstaller, ToolIdentity};

let skill = BundledSkill::single_md(include_str!("../skills/mytool/SKILL.md"));
// The version passed here is stamped into the installed SKILL.md's
// metadata.version — the bundled file needs no version of its own.
let installer = SkillInstaller::new(ToolIdentity::new("mytool", "1.2.0"));
let prod_ctx = ProductionContext::claude()?;
let ctx = prod_ctx.ctx();
let plan = installer.plan(&skill, Scope::Global, false, &ctx)?;
println!("{plan}");                              // dry-run: names every file and destination
let report = installer.apply(&skill, &plan, &ctx)?;
println!("{report}");                            // summary: platform, action, destination, version
```

## Context

`ProductionContext::claude()` is the one-call default for Claude Code tools. It is
equivalent to `ProductionContext::from_env(Platform::Claude)`.

The platform argument passed to `from_env` sets the **default platform binding** for
lock-file and path resolution (which `cmx-lock*.json` file is primary, which install
directory is the default). It does **not** set the config root directory — that is
always `$HOME/.config/context-mixer` — and it does **not** determine which platforms
a skill installs to. Installation targets are resolved at plan time from the cmx config
and existing lock files on the machine.

## One version, declared once

You declare your version exactly once — the string passed to `ToolIdentity`. cmx-core
records it in the lock entry **and** reconciles the installed `SKILL.md` frontmatter's
`metadata.version` to match, automatically, at plan/apply time. You do **not** hand-roll
a frontmatter stamper, and the bundled `SKILL.md` needs no version placeholder — whatever
`metadata.version` it carries (or doesn't) is overwritten with the `ToolIdentity` version
on write.

This closes a gap that used to force every embedder to stamp the frontmatter itself:
the lockfile tracks the version, but readers like `cmx doctor` / `cmx list` parse it back
out of the installed `SKILL.md`. If the two disagree, the skill reports a wrong (or
missing) version. cmx-core now keeps them in lockstep on the community-standard
`metadata.version` key.

The reconciliation is surgical and idempotent: it rewrites only the version line
(preserving folded description blocks, comments, and key order), removes any shadowing
top-level `version:`, and produces byte-identical output on a re-install, so it composes
cleanly with the skip/drift guards below. `cmx-lock.json` / `cmx-lock-<platform>.json`
remain the source of truth for *tracking*; the frontmatter is kept consistent with it.

## `remove()` semantics

`remove()` deletes the installed skill directory and clears the tool's entry from the
platform lock file(s). It intentionally leaves the shared `cmx-lock.json` file on disk
— that file is shared with other tools and cmx itself. The `RemoveReport` display
output notes this explicitly.

## Multi-file bundles

Skills with more than one file build the bundle from `SkillFile::text` entries; relative paths (including subdirectories) are preserved under the installed skill directory:

```rust
let skill = BundledSkill::from_files(vec![
    SkillFile::text("SKILL.md", include_str!("../skill/SKILL.md")),
    SkillFile::text("references/workflows.md", include_str!("../skill/references/workflows.md")),
]);
```

## Testing your integration

The `test-support` feature exposes `test_support::TestContext`, an in-memory context for exercising your init command without touching the real filesystem. Because features unify across a dependency graph, enable it from `[dev-dependencies]` by repeating the dependency with the feature:

```toml
[dependencies]
cmx-core = { version = "0.1" }

[dev-dependencies]
cmx-core = { version = "0.1", features = ["test-support"] }
```

## `TargetAction` is non-exhaustive

`TargetAction` is marked `#[non_exhaustive]`: new variants may be added in future
minor releases without a breaking change. Embedders should render actions via the
`Display` impl on `InstallPlan` and `Report`, or branch on specific variants they care
about with a catch-all `_` arm. The `will_write()` and `is_blocked()` helpers cover
the two most common branching points without requiring exhaustive matching.

## What you get

- **Platform-aware destinations** — knows the skill directories for Claude, Codex, Cursor, Copilot, and ten other agent platforms, at global or project scope (global by default).
- **cmx integration** — on a cmx-managed machine, the skill is registered as a tracked artifact that `cmx doctor`, `cmx list`, and `cmx update` all understand. Without cmx, the install still works and still records a lock entry: the lockfile format is the integration contract, so a later cmx arrival finds everything tracked instead of orphaned.
- **Standardized version guard** — older installed → update; same version with identical content → skip; newer installed → refuse unless forced.
- **Plan/apply** — every mutation previews precisely before it happens; `apply` performs exactly what `plan` reported. Both return `Display`-able types for consistent CLI output across all embedding tools.
- **Drift detection** — `DriftedSkip` targets (same version, content edited locally) are preserved and rendered distinctly in the report so users see "local edits preserved" rather than a silent skip.
- **Uninstall** — `remove()` deletes the installed files and cleans up the lock entry.

The design and fleet-wide roadmap (ports to Python, TypeScript, and beyond) live in [EMBEDDING.md](../EMBEDDING.md).

## License

MIT
