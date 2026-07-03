# cmx-core

Embeddable core for installing agent skills across platforms. Extracted from [cmx](https://github.com/svetzal/context-mixer2), the context-mixer CLI.

CLI tools that ship a companion agent skill use this crate to install it, instead of hand-rolling file copies into hard-wired paths:

```rust
use cmx_core::skill_install::{SkillInstaller, ToolIdentity};

let installer = SkillInstaller::new(ToolIdentity {
    name: "mytool".into(),
    version: "1.2.0".into(),
});
let plan = installer.plan(&bundled_skill, scope)?; // dry-run: names every file and destination
println!("{plan}");
let report = installer.apply(&plan)?;              // copy + lockfile provenance
```

What you get:

- **Platform-aware destinations** — knows the skill directories for Claude, Codex, Cursor, Copilot, and ten other agent platforms, at global or project scope (global by default).
- **cmx integration** — on a cmx-managed machine, the skill is registered as a tracked artifact that `cmx doctor`, `cmx list`, and `cmx update` all understand. Without cmx, the install still works and still records a lock entry: the lockfile format is the integration contract, so a later cmx arrival finds everything tracked instead of orphaned.
- **Standardized version guard** — older installed → update; same version with identical content → skip; newer installed → refuse unless forced.
- **Plan/apply** — every mutation previews precisely before it happens; `apply` performs exactly what `plan` reported.
- **Uninstall** — `remove()` deletes the installed files and cleans up the lock entry.

The design and fleet-wide roadmap (ports to Python, TypeScript, and beyond) live in [EMBEDDING.md](../EMBEDDING.md).

## License

MIT
