//! High-level, embeddable skill-installation API.
//!
//! A tool bundles its companion skill (as [`BundledSkill`]) and calls
//! [`SkillInstaller`] to install, query, or remove it — without knowing about
//! any cmx internals.
//!
//! ```no_run
//! # fn main() -> anyhow::Result<()> {
//! use cmx_core::production::ProductionContext;
//! use cmx_core::skill_install::{BundledSkill, Scope, SkillInstaller, ToolIdentity};
//!
//! // The SKILL.md needs no version of its own — the installer stamps
//! // `metadata.version` from the ToolIdentity below at install time.
//! let skill = BundledSkill::single_md("---\nname: mytool\n---\n# My skill\n");
//! let installer = SkillInstaller::new(ToolIdentity::new("mytool", "1.2.0"));
//! let prod_ctx = ProductionContext::claude()?;
//! let ctx = prod_ctx.ctx();
//! let plan = installer.plan(&skill, Scope::Global, false, &ctx)?;
//! println!("{plan}");
//! let report = installer.apply(&skill, &plan, &ctx)?;
//! println!("{report}");
//! # Ok(())
//! # }
//! ```

mod types;
pub use types::*;

mod display;

mod plan;

mod apply;
mod remove;
mod status;

#[cfg(test)]
mod test_support;

// ---------------------------------------------------------------------------
// SkillInstaller
// ---------------------------------------------------------------------------

/// High-level skill lifecycle manager for embedding tools.
pub struct SkillInstaller {
    tool: ToolIdentity,
}

impl SkillInstaller {
    /// Create a new installer for the given tool identity.
    pub fn new(tool: ToolIdentity) -> Self {
        Self { tool }
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_md_builds_single_skill_md() {
        let skill = BundledSkill::single_md("---\nversion: 1.0.0\n---\n# My skill\n");
        assert_eq!(skill.files.len(), 1);
        assert!(skill.has_skill_md());
        assert_eq!(skill.files[0].rel_path, std::path::PathBuf::from("SKILL.md"));
    }

    #[test]
    fn tool_identity_new_sets_fields() {
        let id = ToolIdentity::new("mytool", "1.2.3");
        assert_eq!(id.name, "mytool");
        assert_eq!(id.version, "1.2.3");
    }

    #[test]
    fn scope_partial_eq() {
        assert_eq!(Scope::Global, Scope::Global);
        assert_eq!(Scope::Local, Scope::Local);
        assert_ne!(Scope::Global, Scope::Local);
    }
}
