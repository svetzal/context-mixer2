# cmx-core

Embeddable core for installing agent skills across platforms. Extracted from [cmx](https://github.com/svetzal/context-mixer2), the context-mixer CLI.

CLI tools that ship a companion agent skill use this crate to install it, instead of hand-rolling file copies into hard-wired paths:

```rust
use cmx_core::production::ProductionContext;
use cmx_core::skill_install::{BundledSkill, Scope, SkillInstaller, ToolIdentity};

let skill = BundledSkill::single_md(include_str!("../skills/mytool/SKILL.md"));
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

## Lockfile is the source of truth

The `cmx-lock.json` / `cmx-lock-<platform>.json` files are the single source of truth
for installed-version tracking. The bundled content (via `BundledSkill`) needs no
version frontmatter stamping — the version comes from `ToolIdentity` passed at
construction time and is recorded in the lock entry.

## `remove()` semantics

`remove()` deletes the installed skill directory and clears the tool's entry from the
platform lock file(s). It intentionally leaves the shared `cmx-lock.json` file on disk
— that file is shared with other tools and cmx itself. The `RemoveReport` display
output notes this explicitly.

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
