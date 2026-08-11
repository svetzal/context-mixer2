//! Embeddable plan/apply installation for generated agents and skills.

use std::cmp::Ordering;
use std::path::PathBuf;

use crate::checksum;
use crate::config;
use crate::context::AppContext;
use crate::error::{CmxError, Result};
use crate::frontmatter;
use crate::lockfile;
use crate::platform::Platform;
use crate::skill_install::{BundledSkill, Scope, SkillInstaller, TargetAction, ToolIdentity};
use crate::targets;
use crate::types::{ArtifactKind, LockEntry, LockSource, SourceEntry, SourceType};

/// Identity recorded for a generated artifact.
#[derive(Debug, Clone)]
pub struct ArtifactIdentity {
    /// Installed artifact name.
    pub name: String,
    /// Semver version recorded in frontmatter and lock files.
    pub version: String,
}

impl ArtifactIdentity {
    /// Construct an artifact identity.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// In-memory artifact ready for installation.
pub enum BundledArtifact {
    /// Markdown agent source. Codex targets are transformed to TOML at install time.
    Agent(String),
    /// Directory-based skill bundle.
    Skill(BundledSkill),
}

impl BundledArtifact {
    /// Construct a markdown agent bundle.
    pub fn agent(markdown: impl Into<String>) -> Self {
        Self::Agent(markdown.into())
    }

    /// Construct a single-file skill bundle.
    pub fn skill_md(markdown: &str) -> Self {
        Self::Skill(BundledSkill::single_md(markdown))
    }

    /// Return this artifact's kind.
    pub fn kind(&self) -> ArtifactKind {
        match self {
            Self::Agent(_) => ArtifactKind::Agent,
            Self::Skill(_) => ArtifactKind::Skill,
        }
    }
}

/// Plan for installing a generated artifact.
pub enum ArtifactInstallPlan {
    /// Agent-specific plan.
    Agent(AgentInstallPlan),
    /// Existing cmx-core skill plan.
    Skill(crate::skill_install::InstallPlan),
}

impl ArtifactInstallPlan {
    /// Whether any target is blocked by a version guard.
    pub fn is_blocked(&self) -> bool {
        match self {
            Self::Agent(p) => p.targets.iter().any(|t| t.action.is_blocked()),
            Self::Skill(p) => p.is_blocked(),
        }
    }
}

/// Per-platform agent plan.
#[derive(Debug)]
pub struct AgentTargetPlan {
    /// Target platform.
    pub platform: Platform,
    /// Destination file.
    pub dest_path: PathBuf,
    /// Version and drift decision.
    pub action: TargetAction,
    /// Bytes that will be installed on this platform.
    installed_bytes: Vec<u8>,
    /// Checksum of those installed bytes.
    installed_checksum: String,
}

/// Complete agent installation plan.
#[derive(Debug)]
pub struct AgentInstallPlan {
    /// Artifact identity.
    pub artifact: ArtifactIdentity,
    /// Install scope.
    pub scope: crate::types::InstallScope,
    /// Checksum of the reconciled markdown source.
    pub source_checksum: String,
    /// Per-platform plans.
    pub targets: Vec<AgentTargetPlan>,
    /// Whether force was requested.
    pub force: bool,
    /// Whether cmx has an explicit managed platform set.
    pub cmx_managed: bool,
}

/// Apply result for either kind of generated artifact.
pub enum ArtifactInstallReport {
    /// Agent apply result.
    Agent(AgentInstallReport),
    /// Skill apply result.
    Skill(crate::skill_install::Report),
}

/// Result of applying an agent plan.
#[derive(Debug)]
pub struct AgentInstallReport {
    /// Artifact identity.
    pub artifact: ArtifactIdentity,
    /// Per-platform outcomes.
    pub targets: Vec<AgentTargetOutcome>,
    /// Whether a canonical cmx source was registered.
    pub source_registered: bool,
}

/// Result for one agent target.
#[derive(Debug)]
pub struct AgentTargetOutcome {
    /// Target platform.
    pub platform: Platform,
    /// Destination file.
    pub dest_path: PathBuf,
    /// Applied decision.
    pub action: TargetAction,
}

/// Installer for generated agent or skill artifacts.
pub struct ArtifactInstaller {
    artifact: ArtifactIdentity,
}

impl ArtifactInstaller {
    /// Construct an installer.
    pub fn new(artifact: ArtifactIdentity) -> Self {
        Self { artifact }
    }

    /// Compute a dry-run plan without writing files.
    pub fn plan(
        &self,
        bundle: &BundledArtifact,
        scope: Scope,
        force: bool,
        ctx: &AppContext<'_>,
    ) -> Result<ArtifactInstallPlan> {
        match bundle {
            BundledArtifact::Skill(skill) => {
                SkillInstaller::new(ToolIdentity::new(&self.artifact.name, &self.artifact.version))
                    .plan(skill, scope, force, ctx)
                    .map(ArtifactInstallPlan::Skill)
            }
            BundledArtifact::Agent(markdown) => {
                self.plan_agent(markdown, scope, force, ctx).map(ArtifactInstallPlan::Agent)
            }
        }
    }

    /// Apply a previously computed plan.
    pub fn apply(
        &self,
        bundle: &BundledArtifact,
        plan: &ArtifactInstallPlan,
        ctx: &AppContext<'_>,
    ) -> anyhow::Result<ArtifactInstallReport> {
        match (bundle, plan) {
            (BundledArtifact::Skill(skill), ArtifactInstallPlan::Skill(plan)) => {
                SkillInstaller::new(ToolIdentity::new(&self.artifact.name, &self.artifact.version))
                    .apply(skill, plan, ctx)
                    .map(ArtifactInstallReport::Skill)
            }
            (BundledArtifact::Agent(markdown), ArtifactInstallPlan::Agent(plan)) => self
                .apply_agent(markdown, plan, ctx)
                .map(ArtifactInstallReport::Agent)
                .map_err(Into::into),
            _ => Err(anyhow::anyhow!("artifact kind does not match install plan")),
        }
    }

    fn plan_agent(
        &self,
        markdown: &str,
        scope: Scope,
        force: bool,
        ctx: &AppContext<'_>,
    ) -> Result<AgentInstallPlan> {
        let source = frontmatter::reconcile_document_version(markdown, &self.artifact.version);
        let source_checksum = checksum::checksum_bytes(source.as_bytes());
        let install_scope = scope.to_install_scope();
        let platforms = targets::resolve_targets(None, ArtifactKind::Agent, install_scope, ctx)?;
        let cmx_managed = config::managed_platforms(ctx.fs, ctx.paths)?.is_some();
        let mut target_plans = Vec::new();

        for platform in platforms {
            let pv = ctx.paths.with_platform(platform);
            let dest_path = pv.require_installed_artifact_path(
                ArtifactKind::Agent,
                &self.artifact.name,
                install_scope,
            )?;
            let installed = if platform.transforms_agent_to_toml() {
                crate::agent::markdown_to_codex_toml(&source, &self.artifact.name).into_bytes()
            } else {
                source.as_bytes().to_vec()
            };
            let installed_checksum = checksum::checksum_bytes(&installed);
            let lock = lockfile::load(install_scope, ctx.fs, &pv)?;
            let action = lock.packages.get(&self.artifact.name).map_or(
                Ok(TargetAction::Install),
                |entry| {
                    decide_agent_action(
                        entry,
                        &self.artifact.version,
                        &installed_checksum,
                        force,
                        &dest_path,
                        ctx,
                    )
                },
            )?;
            target_plans.push(AgentTargetPlan {
                platform,
                dest_path,
                action,
                installed_bytes: installed,
                installed_checksum,
            });
        }

        Ok(AgentInstallPlan {
            artifact: self.artifact.clone(),
            scope: install_scope,
            source_checksum,
            targets: target_plans,
            force,
            cmx_managed,
        })
    }

    fn apply_agent(
        &self,
        markdown: &str,
        plan: &AgentInstallPlan,
        ctx: &AppContext<'_>,
    ) -> Result<AgentInstallReport> {
        if plan.targets.iter().any(|target| target.action.is_blocked()) {
            return Err(CmxError::VersionGuard {
                tool: self.artifact.name.clone(),
            });
        }
        let source = frontmatter::reconcile_document_version(markdown, &self.artifact.version);
        if checksum::checksum_bytes(source.as_bytes()) != plan.source_checksum {
            return Err(CmxError::Drift {
                tool: self.artifact.name.clone(),
            });
        }

        let installed_at = ctx.clock.now().to_rfc3339();
        let mut outcomes = Vec::new();
        for target in &plan.targets {
            if target.action.will_write() {
                if let Some(parent) = target.dest_path.parent() {
                    ctx.fs.create_dir_all(parent)?;
                }
                ctx.fs.write_bytes(&target.dest_path, &target.installed_bytes)?;
                let pv = ctx.paths.with_platform(target.platform);
                lockfile::mutate(plan.scope, ctx.fs, &pv, |lock| {
                    lock.packages.insert(
                        self.artifact.name.clone(),
                        LockEntry::new(
                            ArtifactKind::Agent,
                            Some(self.artifact.version.clone()),
                            LockSource::new(
                                format!("bundled:{}", self.artifact.name),
                                format!("agents/{}.md", self.artifact.name),
                            ),
                            plan.source_checksum.clone(),
                            target.installed_checksum.clone(),
                            installed_at.clone(),
                        ),
                    );
                })?;
            }
            outcomes.push(AgentTargetOutcome {
                platform: target.platform,
                dest_path: target.dest_path.clone(),
                action: target.action.clone(),
            });
        }

        let source_registered = if plan.cmx_managed {
            let home =
                config::resolve_artifact_home(&config::load_config(ctx.fs, ctx.paths)?, ctx.paths);
            let materialized = home.join("agents").join(format!("{}.md", self.artifact.name));
            if let Some(parent) = materialized.parent() {
                ctx.fs.create_dir_all(parent)?;
            }
            ctx.fs.write(&materialized, &source)?;
            let source_name = format!("bundled:{}", self.artifact.name);
            config::mutate_sources(ctx.fs, ctx.paths, |sources| -> Result<()> {
                sources.sources.insert(
                    source_name,
                    SourceEntry {
                        source_type: SourceType::Local,
                        path: Some(materialized),
                        url: None,
                        local_clone: None,
                        branch: None,
                        last_updated: Some(installed_at),
                    },
                );
                Ok(())
            })?;
            true
        } else {
            false
        };

        Ok(AgentInstallReport {
            artifact: self.artifact.clone(),
            targets: outcomes,
            source_registered,
        })
    }
}

fn decide_agent_action(
    entry: &LockEntry,
    bundled_version: &str,
    installed_checksum: &str,
    force: bool,
    dest: &std::path::Path,
    ctx: &AppContext<'_>,
) -> Result<TargetAction> {
    let comparison = match entry.version.as_deref() {
        None => Ordering::Less,
        Some(installed) => {
            match (semver::Version::parse(installed), semver::Version::parse(bundled_version)) {
                (Ok(a), Ok(b)) => a.cmp(&b),
                _ if installed == bundled_version => Ordering::Equal,
                _ => Ordering::Less,
            }
        }
    };
    match comparison {
        Ordering::Less => Ok(TargetAction::Update {
            from: entry.version.clone(),
        }),
        Ordering::Greater if force => Ok(TargetAction::Downgrade {
            from: entry.version.clone().unwrap_or_else(|| "unknown".to_string()),
        }),
        Ordering::Greater => Ok(TargetAction::RefuseNewer {
            installed: entry.version.clone().unwrap_or_else(|| "unknown".to_string()),
        }),
        Ordering::Equal if !ctx.fs.exists(dest) => Ok(TargetAction::Install),
        Ordering::Equal => {
            let current = checksum::checksum_file(dest, ctx.fs)?;
            if current == installed_checksum {
                Ok(TargetAction::Skip)
            } else if force {
                Ok(TargetAction::Update {
                    from: entry.version.clone(),
                })
            } else {
                Ok(TargetAction::DriftedSkip {
                    installed: entry.version.clone().unwrap_or_else(|| "unknown".to_string()),
                })
            }
        }
    }
}

impl std::fmt::Display for ArtifactInstallPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skill(plan) => plan.fmt(f),
            Self::Agent(plan) => {
                writeln!(f, "Install plan for {} v{}", plan.artifact.name, plan.artifact.version)?;
                for target in &plan.targets {
                    writeln!(
                        f,
                        "  {} → {} ({})",
                        target.platform,
                        target.dest_path.display(),
                        action_label(&target.action)
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::fmt::Display for ArtifactInstallReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Skill(report) => report.fmt(f),
            Self::Agent(report) => {
                writeln!(f, "Installed {} v{}", report.artifact.name, report.artifact.version)?;
                for target in &report.targets {
                    writeln!(
                        f,
                        "  {} → {} ({})",
                        target.platform,
                        target.dest_path.display(),
                        action_label(&target.action)
                    )?;
                }
                Ok(())
            }
        }
    }
}

fn action_label(action: &TargetAction) -> String {
    match action {
        TargetAction::Install => "install".to_string(),
        TargetAction::Update { from } => format!("update from {}", from.as_deref().unwrap_or("?")),
        TargetAction::Skip => "skip (up to date)".to_string(),
        TargetAction::DriftedSkip { installed } => format!("skip (drifted from {installed})"),
        TargetAction::RefuseNewer { installed } => {
            format!("refuse (installed {installed} is newer)")
        }
        TargetAction::Downgrade { from } => format!("downgrade from {from}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;
    use crate::gateway::Filesystem;
    use crate::platform::Platform;
    use crate::test_support::TestContext;
    use crate::types::{CmxConfig, InstallScope};

    #[test]
    fn agent_plan_and_apply_write_markdown_and_lock() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let bundle =
            BundledArtifact::agent("---\nname: helper\ndescription: Helps\n---\nBe helpful.\n");
        let installer = ArtifactInstaller::new(ArtifactIdentity::new("helper", "1.0.0"));
        let plan = installer.plan(&bundle, Scope::Global, false, &ctx).unwrap();
        installer.apply(&bundle, &plan, &ctx).unwrap();
        let path = t
            .paths
            .require_installed_artifact_path(ArtifactKind::Agent, "helper", InstallScope::Global)
            .unwrap();
        assert!(t.fs.file_exists(&path));
        assert!(
            lockfile::load(InstallScope::Global, &t.fs, &t.paths)
                .unwrap()
                .packages
                .contains_key("helper")
        );
    }

    #[test]
    fn codex_agent_target_writes_toml() {
        let t = TestContext::new();
        config::save_config(
            &CmxConfig {
                platforms: vec![Platform::Codex],
                ..Default::default()
            },
            &t.fs,
            &t.paths,
        )
        .unwrap();
        let ctx = t.ctx();
        let bundle =
            BundledArtifact::agent("---\nname: helper\ndescription: Helps\n---\nBe helpful.\n");
        let installer = ArtifactInstaller::new(ArtifactIdentity::new("helper", "1.0.0"));
        let plan = installer.plan(&bundle, Scope::Global, false, &ctx).unwrap();
        installer.apply(&bundle, &plan, &ctx).unwrap();

        let codex = t.paths.with_platform(Platform::Codex);
        let path = codex
            .require_installed_artifact_path(ArtifactKind::Agent, "helper", InstallScope::Global)
            .unwrap();
        let contents = t.fs.read_to_string(&path).unwrap();
        assert!(path.ends_with("helper.toml"));
        assert!(contents.contains("developer_instructions = \"Be helpful.\""));
    }
}
