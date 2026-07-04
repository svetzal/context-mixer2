use std::fmt;

use cmx_core::skill_install::TargetAction;
use cmx_core::types::InstallScope;
use serde_json::{Value, json};

use crate::init::InitOutcome;

impl fmt::Display for InitOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Delegate to cmx-core's own Display impls — no hand-rolled
            // per-target rendering (see EMBEDDING.md / cmx-core README).
            InitOutcome::Installed(report) => write!(f, "{report}"),
            InitOutcome::Removed(report) => write!(f, "{report}"),
            InitOutcome::Blocked { reasons } => {
                writeln!(f, "cmx init blocked:")?;
                for reason in reasons {
                    writeln!(f, "  {reason}")?;
                }
                Ok(())
            }
        }
    }
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

/// Build the machine-readable `--json` shape for `cmx init` / `cmx init --remove`.
pub fn init_json(outcome: &InitOutcome) -> Value {
    match outcome {
        InitOutcome::Installed(report) => {
            let targets: Vec<Value> = report
                .targets
                .iter()
                .map(|t| {
                    json!({
                        "platform": t.platform.to_string(),
                        "action": action_label(&t.action),
                        "dest": t.dest_dir.display().to_string(),
                        "files_written": t.files_written,
                        "installed_checksum": t.installed_checksum,
                    })
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
        InitOutcome::Blocked { reasons } => json!({
            "tool": "cmx",
            "status": "blocked",
            "reasons": reasons,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::platform::Platform;
    use cmx_core::skill_install::{Report, TargetOutcome, ToolIdentity};
    use std::path::PathBuf;

    fn sample_report() -> Report {
        Report {
            tool: ToolIdentity::new("cmx", "2.10.2"),
            scope: InstallScope::Global,
            targets: vec![TargetOutcome {
                platform: Platform::Claude,
                dest_dir: PathBuf::from("/home/u/.claude/skills/cmx"),
                action: TargetAction::Install,
                files_written: 1,
                installed_checksum: Some("sha256:abc".to_string()),
            }],
            source_registered: false,
        }
    }

    #[test]
    fn display_installed_mentions_tool_and_action() {
        let outcome = InitOutcome::Installed(sample_report());
        let out = outcome.to_string();
        assert!(out.contains("cmx"));
        assert!(out.contains("install"));
    }

    #[test]
    fn display_blocked_lists_reasons() {
        let outcome = InitOutcome::Blocked {
            reasons: vec!["claude: installed version 99.0.0 is newer".to_string()],
        };
        let out = outcome.to_string();
        assert!(out.contains("blocked"));
        assert!(out.contains("99.0.0"));
    }

    #[test]
    fn init_json_installed_has_expected_keys() {
        let outcome = InitOutcome::Installed(sample_report());
        let value = init_json(&outcome);
        assert_eq!(value["tool"], "cmx");
        assert_eq!(value["scope"], "global");
        assert_eq!(value["targets"][0]["platform"], "claude");
        assert_eq!(value["targets"][0]["action"], "install");
        assert_eq!(value["targets"][0]["files_written"], 1);
    }

    #[test]
    fn init_json_blocked_has_status() {
        let outcome = InitOutcome::Blocked {
            reasons: vec!["reason one".to_string()],
        };
        let value = init_json(&outcome);
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["reasons"][0], "reason one");
    }
}
