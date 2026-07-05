use std::fmt;

use cmx_core::platform::Platform;
use cmx_core::skill_install::{InstallPlan, Report, TargetAction};
use cmx_core::types::InstallScope;
use serde_json::{Value, json};

use crate::init::InitOutcome;

struct ActionCounts {
    installed: usize,
    updated: usize,
    skipped_up_to_date: usize,
    skipped_drifted: usize,
    skipped_newer: usize,
}

impl ActionCounts {
    fn from_actions<'a>(actions: impl IntoIterator<Item = &'a TargetAction>) -> Self {
        let mut counts = Self {
            installed: 0,
            updated: 0,
            skipped_up_to_date: 0,
            skipped_drifted: 0,
            skipped_newer: 0,
        };

        for action in actions {
            match action {
                TargetAction::Install => counts.installed += 1,
                TargetAction::Update { .. } | TargetAction::Downgrade { .. } => counts.updated += 1,
                TargetAction::Skip => counts.skipped_up_to_date += 1,
                TargetAction::DriftedSkip { .. } => counts.skipped_drifted += 1,
                TargetAction::RefuseNewer { .. } => counts.skipped_newer += 1,
                _ => {}
            }
        }

        counts
    }
}

impl fmt::Display for InitOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InitOutcome::Installed(report) => render_report(f, report),
            InitOutcome::Removed(report) => write!(f, "{report}"),
            InitOutcome::Blocked { plan, reasons } => render_blocked(f, plan, reasons),
        }
    }
}

fn render_report(f: &mut fmt::Formatter<'_>, report: &Report) -> fmt::Result {
    let counts = ActionCounts::from_actions(report.targets.iter().map(|target| &target.action));
    writeln!(f, "{}", report_summary(&counts))?;
    if counts.skipped_up_to_date > 0
        && (counts.installed > 0 || counts.updated > 0 || counts.skipped_drifted > 0)
    {
        writeln!(
            f,
            "{} already up to date.",
            count_label(counts.skipped_up_to_date, "copy", "copies")
        )?;
    }
    for target in &report.targets {
        writeln!(
            f,
            "  {} → {} ({})",
            target.platform,
            target.dest_dir.display(),
            describe_action(&target.action)
        )?;
    }
    if report.source_registered {
        writeln!(f, "  (registered as cmx source)")?;
    }
    Ok(())
}

fn render_blocked(
    f: &mut fmt::Formatter<'_>,
    plan: &InstallPlan,
    reasons: &[String],
) -> fmt::Result {
    let counts = ActionCounts::from_actions(plan.targets.iter().map(|target| &target.action));
    writeln!(f, "cmx init blocked.")?;
    if counts.skipped_newer > 0 {
        writeln!(
            f,
            "Skipped {} newer {} (use --force).",
            counts.skipped_newer,
            copy_word(counts.skipped_newer)
        )?;
    }
    for target in &plan.targets {
        writeln!(
            f,
            "  {} → {} ({})",
            target.platform,
            target.dest_dir.display(),
            describe_action(&target.action)
        )?;
    }
    for reason in reasons {
        writeln!(f, "  {reason}")?;
    }
    Ok(())
}

fn report_summary(counts: &ActionCounts) -> String {
    let mut lines = Vec::new();
    if counts.installed > 0 {
        lines.push(format!("Installed {}.", count_label(counts.installed, "copy", "copies")));
    }
    if counts.updated > 0 {
        lines.push(format!("Updated {}.", count_label(counts.updated, "copy", "copies")));
    }
    if counts.skipped_drifted > 0 {
        lines.push(format!(
            "Skipped {} drifted {} (use --force).",
            counts.skipped_drifted,
            copy_word(counts.skipped_drifted)
        ));
    }
    if lines.is_empty() {
        if counts.skipped_up_to_date > 0 {
            format!(
                "Already up to date on {}.",
                count_label(counts.skipped_up_to_date, "copy", "copies")
            )
        } else if counts.skipped_newer > 0 {
            format!(
                "Skipped {} newer {} (use --force).",
                counts.skipped_newer,
                copy_word(counts.skipped_newer)
            )
        } else {
            "No copies changed.".to_string()
        }
    } else {
        lines.join(" ")
    }
}

fn describe_action(action: &TargetAction) -> String {
    match action {
        TargetAction::Install => "install".to_string(),
        TargetAction::Update { from } => {
            format!("update from {}", from.as_deref().unwrap_or("?"))
        }
        TargetAction::Skip => "skip (up to date)".to_string(),
        TargetAction::DriftedSkip { installed } => format!(
            "skip (drifted from {installed}; re-run with --force to overwrite, or 'cmx skill promote cmx' to keep the edits)"
        ),
        TargetAction::RefuseNewer { installed } => {
            format!("skip (installed {installed} is newer; use --force to override)")
        }
        TargetAction::Downgrade { from } => format!("downgrade from {from}"),
        _ => "unknown".to_string(),
    }
}

fn count_label(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

fn copy_word(count: usize) -> &'static str {
    if count == 1 { "copy" } else { "copies" }
}

fn scope_label(scope: InstallScope) -> &'static str {
    match scope {
        InstallScope::Global => "global",
        InstallScope::Local => "local",
    }
}

/// Map a `TargetAction` to the short label used in the `--json` output.
/// `TargetAction` is `#[non_exhaustive]`, so this always needs a catch-all.
fn action_label(action: &TargetAction) -> &'static str {
    match action {
        TargetAction::Install => "install",
        TargetAction::Update { .. } => "update",
        TargetAction::Skip => "skip",
        TargetAction::DriftedSkip { .. } => "drifted_skip",
        TargetAction::RefuseNewer { .. } => "refuse_newer",
        TargetAction::Downgrade { .. } => "downgrade",
        _ => "unknown",
    }
}

fn status_label(action: &TargetAction) -> &'static str {
    match action {
        TargetAction::Install => "installed",
        TargetAction::Update { .. } | TargetAction::Downgrade { .. } => "updated",
        TargetAction::DriftedSkip { .. } => "skipped_drifted",
        TargetAction::RefuseNewer { .. } => "skipped_newer",
        TargetAction::Skip => "skipped_up_to_date",
        _ => "unknown",
    }
}

fn target_json(
    platform: Platform,
    dest_dir: &std::path::Path,
    action: &TargetAction,
    files_written: usize,
    installed_checksum: Option<&str>,
) -> Value {
    json!({
        "platform": platform.to_string(),
        "action": action_label(action),
        "status": status_label(action),
        "dest": dest_dir.display().to_string(),
        "files_written": files_written,
        "installed_checksum": installed_checksum,
    })
}

/// Build the machine-readable `--json` shape for `cmx init` / `cmx init --remove`.
pub fn init_json(outcome: &InitOutcome) -> Value {
    match outcome {
        InitOutcome::Installed(report) => {
            let targets: Vec<Value> = report
                .targets
                .iter()
                .map(|target| {
                    target_json(
                        target.platform,
                        &target.dest_dir,
                        &target.action,
                        target.files_written,
                        target.installed_checksum.as_deref(),
                    )
                })
                .collect();
            json!({
                "tool": report.tool.name,
                "version": report.tool.version,
                "scope": scope_label(report.scope),
                "source_registered": report.source_registered,
                "targets": targets,
            })
        }
        InitOutcome::Removed(report) => json!({
            "tool": report.tool_name,
            "scope": scope_label(report.scope),
            "removed_dirs": report.removed_dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>(),
            "platforms_cleared": report.platforms_cleared.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "source_unregistered": report.source_unregistered,
            "was_on_disk": report.was_on_disk,
            "was_tracked": report.was_tracked,
        }),
        InitOutcome::Blocked { plan, reasons } => {
            let targets: Vec<Value> = plan
                .targets
                .iter()
                .map(|target| {
                    target_json(target.platform, &target.dest_dir, &target.action, 0, None)
                })
                .collect();
            json!({
                "tool": plan.tool.name,
                "version": plan.tool.version,
                "scope": scope_label(plan.scope),
                "status": "blocked",
                "targets": targets,
                "reasons": reasons,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::skill_install::{
        InstallPlan, PlannedFile, TargetOutcome, TargetPlan, ToolIdentity,
    };
    use std::path::PathBuf;

    fn sample_report(action: TargetAction) -> Report {
        let files_written = usize::from(action.will_write());
        let installed_checksum = action.will_write().then(|| "sha256:abc".to_string());
        Report {
            tool: ToolIdentity::new("cmx", "2.10.2"),
            scope: InstallScope::Global,
            targets: vec![TargetOutcome {
                platform: Platform::Claude,
                dest_dir: PathBuf::from("/home/u/.claude/skills/cmx"),
                action,
                files_written,
                installed_checksum,
            }],
            source_registered: false,
        }
    }

    fn blocked_plan(action: TargetAction) -> InstallPlan {
        InstallPlan {
            tool: ToolIdentity::new("cmx", "2.10.2"),
            scope: InstallScope::Global,
            source_checksum: "sha256:source".to_string(),
            cmx_present: false,
            force: false,
            targets: vec![TargetPlan {
                platform: Platform::Claude,
                scope: InstallScope::Global,
                dest_dir: PathBuf::from("/home/u/.claude/skills/cmx"),
                files: vec![PlannedFile {
                    rel_path: PathBuf::from("SKILL.md"),
                    dest_path: PathBuf::from("/home/u/.claude/skills/cmx/SKILL.md"),
                }],
                action,
                cmx_managed: false,
            }],
        }
    }

    #[test]
    fn display_installed_uses_count_based_summary() {
        let outcome = InitOutcome::Installed(sample_report(TargetAction::Update {
            from: Some("2.10.1".to_string()),
        }));
        let out = outcome.to_string();
        assert!(out.contains("Updated 1 copy."));
        assert!(!out.contains("Installed cmx v2.10.2"));
    }

    #[test]
    fn display_drifted_skip_mentions_force_and_promote() {
        let outcome = InitOutcome::Installed(sample_report(TargetAction::DriftedSkip {
            installed: "2.10.2".to_string(),
        }));
        let out = outcome.to_string();
        assert!(out.contains("Skipped 1 drifted copy (use --force)."));
        assert!(out.contains("cmx skill promote cmx"));
    }

    #[test]
    fn display_blocked_lists_target_and_reason() {
        let outcome = InitOutcome::Blocked {
            plan: blocked_plan(TargetAction::RefuseNewer {
                installed: "99.0.0".to_string(),
            }),
            reasons: vec!["claude: installed version 99.0.0 is newer".to_string()],
        };
        let out = outcome.to_string();
        assert!(out.contains("blocked"));
        assert!(out.contains("99.0.0"));
        assert!(out.contains("use --force"));
    }

    #[test]
    fn init_json_installed_has_expected_keys() {
        let outcome = InitOutcome::Installed(sample_report(TargetAction::Install));
        let value = init_json(&outcome);
        assert_eq!(value["tool"], "cmx");
        assert_eq!(value["scope"], "global");
        assert_eq!(value["targets"][0]["platform"], "claude");
        assert_eq!(value["targets"][0]["action"], "install");
        assert_eq!(value["targets"][0]["status"], "installed");
        assert_eq!(value["targets"][0]["files_written"], 1);
    }

    #[test]
    fn init_json_drifted_skip_uses_skipped_drifted_status() {
        let outcome = InitOutcome::Installed(sample_report(TargetAction::DriftedSkip {
            installed: "2.10.2".to_string(),
        }));
        let value = init_json(&outcome);
        assert_eq!(value["targets"][0]["action"], "drifted_skip");
        assert_eq!(value["targets"][0]["status"], "skipped_drifted");
        assert_eq!(value["targets"][0]["installed_checksum"], Value::Null);
    }

    #[test]
    fn init_json_blocked_has_status_and_targets() {
        let outcome = InitOutcome::Blocked {
            plan: blocked_plan(TargetAction::RefuseNewer {
                installed: "99.0.0".to_string(),
            }),
            reasons: vec!["reason one".to_string()],
        };
        let value = init_json(&outcome);
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["targets"][0]["status"], "skipped_newer");
        assert_eq!(value["reasons"][0], "reason one");
    }
}
