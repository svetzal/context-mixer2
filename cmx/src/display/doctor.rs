use std::fmt;

use crate::doctor::DoctorReport;
use crate::table::Table;

/// Build the artifact table from the given grouped logical artifacts — one row
/// per skill, the Tools column listing every tool it's installed for.
fn doctor_artifact_table(artifacts: &[&crate::doctor::DoctorArtifact]) -> Table {
    Table {
        headers: vec![
            "Type", "Name", "Scope", "State", "Version", "Source", "Tools",
        ],
        padded_cols: 6,
        rows: artifacts
            .iter()
            .map(|a| {
                let tools = if a.tools.is_empty() {
                    "-".to_string()
                } else {
                    a.tools.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
                };
                // When copies agree show the single version; when they diverge
                // name the skew (`3.2.0 / 3.3.0`) rather than an opaque `-`.
                let version = a.version.clone().unwrap_or_else(|| {
                    if a.versions.len() > 1 {
                        a.versions.join(" / ")
                    } else {
                        "-".to_string()
                    }
                });
                let mut cells = vec![
                    a.kind.to_string(),
                    a.name.clone(),
                    a.scope.label().to_string(),
                    a.state.label().to_string(),
                    version,
                    a.source.clone().unwrap_or_else(|| "-".to_string()),
                    tools,
                ];
                if a.diverged {
                    cells.push("(diverged)".to_string());
                }
                cells
            })
            .collect(),
    }
}

/// Build the "Missing" table from lock entries with no file on disk.
fn doctor_missing_table(report: &DoctorReport) -> Table {
    Table {
        headers: vec!["Type", "Name", "Scope", "Platform"],
        padded_cols: 4,
        rows: report
            .missing
            .iter()
            .map(|m| {
                vec![
                    m.kind.to_string(),
                    m.name.clone(),
                    m.scope.label().to_string(),
                    m.platform.to_string(),
                ]
            })
            .collect(),
    }
}

/// Honest next-step hints — one line per state that actually occurs, referring
/// only to capabilities that exist today.
fn doctor_hints(c: &crate::doctor::StateCounts) -> String {
    let mut lines = Vec::new();
    if c.orphaned > 0 {
        lines.push(format!(
            "  • {} orphaned artifact(s) have no source (hand-authored) — `cmx <kind> adopt <name>` (or `cmx doctor --adopt-all`) canonicalizes them into the home.",
            c.orphaned
        ));
    }
    if c.untracked > 0 {
        lines.push(format!(
            "  • {} untracked artifact(s) are installed but a registered source provides them — `cmx <kind> install <name>` records provenance and tracks them.",
            c.untracked
        ));
    }
    if c.drifted > 0 {
        lines.push(format!(
            "  • {} drifted artifact(s) differ from their lock file — inspect with `cmx info <name>`.",
            c.drifted
        ));
    }
    if c.missing > 0 {
        lines.push(format!(
            "  • {} missing artifact(s) are recorded in a lock file but gone from disk — `cmx <kind> uninstall <name>` clears the stale entry (or reinstall if the source still has it).",
            c.missing
        ));
    }
    if c.diverged > 0 {
        lines.push(format!(
            "  • {} artifact(s) diverge across their install locations (different version or state). For a skill tracked from a source or the home, make one copy canonical and re-project: `cmx skill promote <name>` (push in-place edits into the home) or `cmx skill update <name> --force` (restore from source). For source-less or external skills, reconcile between locations with `cmx skill sync <name>` (newest version wins, or `--from <platform>`). Inspect with `cmx skill diff <name>`.",
            c.diverged
        ));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", lines.join("\n"))
    }
}

/// Per-location breakdown for each shown diverged artifact, naming which copy
/// carries which version (and state, when states differ too). Delegates all
/// filtering/grouping to the pure `doctor::divergence_details` core; this
/// function only renders the returned structs into output lines.
fn doctor_divergence_details(
    shown: &[&crate::doctor::DoctorArtifact],
    rows: &[crate::doctor::DoctorRow],
) -> String {
    let details = crate::doctor::divergence_details(shown, rows);
    let mut lines = Vec::new();
    for d in &details {
        let parts: Vec<String> = d
            .members
            .iter()
            .map(|m| {
                let ver = m.version.as_deref().unwrap_or("unversioned");
                if d.states_differ {
                    format!("{} @ {ver} ({})", m.location.display(), m.state_label)
                } else {
                    format!("{} @ {ver}", m.location.display())
                }
            })
            .collect();
        lines.push(format!("  • {} diverges: {}", d.name, parts.join(", ")));
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("\n{}\n", lines.join("\n"))
    }
}

impl fmt::Display for DoctorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scope_desc = if self.included_local {
            "global + project scope"
        } else {
            "global scope"
        };
        let platform_note = if self.scoped_to_managed {
            format!("{} managed platform(s) surveyed", self.surveyed_platforms)
        } else {
            format!("{} platforms surveyed", self.surveyed_platforms)
        };
        writeln!(f, "cmx doctor — {scope_desc}, {platform_note}.\n")?;

        // By default `doctor` shows only what needs attention — it's a doctor.
        // `--all` shows the full inventory.
        let shown: Vec<&crate::doctor::DoctorArtifact> = if self.show_all {
            self.artifacts.iter().collect()
        } else {
            self.artifacts.iter().filter(|a| DoctorReport::is_problem(a)).collect()
        };

        if !shown.is_empty() {
            writeln!(
                f,
                "{}",
                if self.show_all {
                    "Installed artifacts:"
                } else {
                    "Needs attention:"
                }
            )?;
            write!(f, "{}", doctor_artifact_table(&shown).render())?;
        }

        if !self.missing.is_empty() {
            if !shown.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Missing (in a lock file, absent on disk):")?;
            write!(f, "{}", doctor_missing_table(self).render())?;
        }

        if shown.is_empty() && self.missing.is_empty() {
            if self.artifacts.is_empty() {
                writeln!(f, "Nothing installed — your system is clean.")?;
            } else if self.show_all {
                writeln!(f, "No artifacts found.")?;
            } else {
                writeln!(
                    f,
                    "No problems — everything cmx manages is healthy. (`--all` shows the full inventory.)"
                )?;
            }
        }

        let c = self.counts();
        writeln!(
            f,
            "\nSummary: {} tracked, {} drifted, {} untracked, {} orphaned, {} external, {} missing · {} diverged.",
            c.tracked, c.drifted, c.untracked, c.orphaned, c.external, c.missing, c.diverged
        )?;
        write!(f, "{}", doctor_hints(&c))?;
        write!(f, "{}", doctor_divergence_details(&shown, &self.rows))
    }
}

#[cfg(test)]
mod tests {
    use crate::doctor::{ArtifactState, DoctorArtifact, DoctorReport, MissingRow};
    use crate::platform::Platform;
    use crate::types::{ArtifactKind, InstallScope};
    use std::path::PathBuf;

    fn orphan_artifact(name: &str) -> DoctorArtifact {
        DoctorArtifact {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            scope: InstallScope::Global,
            state: ArtifactState::Orphaned,
            version: Some("1.0.0".to_string()),
            versions: vec!["1.0.0".to_string()],
            tools: vec![Platform::Claude],
            source: None,
            locations: vec![PathBuf::from("/home/u/.claude/skills")],
            diverged: false,
        }
    }

    #[test]
    fn doctor_report_clean_system_message() {
        let r = DoctorReport::default();
        let out = r.to_string();
        assert!(out.contains("Nothing installed"), "clean message: {out}");
        assert!(out.contains("global scope"), "default scope description");
    }

    #[test]
    fn doctor_report_lists_artifacts_and_summary() {
        let r = DoctorReport {
            rows: vec![],
            artifacts: vec![orphan_artifact("my-skill")],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("Installed artifacts:"));
        assert!(out.contains("my-skill"));
        assert!(out.contains("orphaned"));
        assert!(out.contains("1 orphaned"), "summary tallies orphans: {out}");
        assert!(out.contains("have no source"), "orphan hint present");
        assert!(out.contains("adopt"), "orphan hint points at adopt");
    }

    #[test]
    fn doctor_report_lists_tools_for_multi_tool_artifact() {
        // One skill installed for two tools is ONE row listing both — not "dup".
        let r = DoctorReport {
            rows: vec![],
            artifacts: vec![DoctorArtifact {
                kind: ArtifactKind::Skill,
                name: "clipboard".to_string(),
                scope: InstallScope::Global,
                state: ArtifactState::Tracked,
                version: Some("1.0.0".to_string()),
                versions: vec!["1.0.0".to_string()],
                tools: vec![Platform::Claude, Platform::Codex],
                source: Some("home".to_string()),
                locations: vec![PathBuf::from("/a"), PathBuf::from("/b")],
                diverged: false,
            }],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("clipboard"));
        assert!(out.contains("claude, codex"), "tools listed in one row: {out}");
        assert!(out.contains("home"), "source provenance shown: {out}");
        assert!(!out.contains("(diverged)"), "consistent copies carry no diverged marker");
        assert!(out.contains("1 tracked"), "counted once, not per-location");
    }

    #[test]
    fn doctor_report_missing_section_and_scope_label() {
        let r = DoctorReport {
            rows: vec![],
            artifacts: vec![],
            missing: vec![MissingRow {
                kind: ArtifactKind::Skill,
                name: "ghost".to_string(),
                scope: InstallScope::Global,
                platform: Platform::Pi,
            }],
            included_local: true,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
        };
        let out = r.to_string();
        assert!(out.contains("global + project scope"), "local-included scope label");
        assert!(out.contains("Missing (in a lock file"));
        assert!(out.contains("ghost"));
        assert!(out.contains("pi"));
        assert!(out.contains("1 missing"));
    }

    #[test]
    fn doctor_report_flags_diverged_artifact() {
        let mut a = orphan_artifact("skew");
        a.diverged = true;
        let r = DoctorReport {
            rows: vec![],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
        };
        let out = r.to_string();
        assert!(out.contains("(diverged)"), "diverged marker rendered: {out}");
        assert!(out.contains("1 diverged"));
        assert!(out.contains("diverge across their install locations"), "diverged hint present");
    }

    #[test]
    fn doctor_report_names_version_skew() {
        // A version-diverged artifact: no single agreed version, but the distinct
        // versions are shown (`3.2.0 / 3.3.0`) rather than an opaque `-`.
        let mut a = orphan_artifact("hopper-coordinator");
        a.diverged = true;
        a.version = None;
        a.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = DoctorReport {
            rows: vec![],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("3.2.0 / 3.3.0"), "version skew named in the table: {out}");
        assert!(out.contains("(diverged)"), "still flagged diverged: {out}");
    }

    #[test]
    fn doctor_details_name_each_locations_version() {
        use crate::doctor::{ArtifactState, DoctorRow};
        use crate::platform::Platform;

        let mk_row = |loc: &str, ver: &str, platform| DoctorRow {
            kind: ArtifactKind::Skill,
            name: "hopper-coordinator".to_string(),
            scope: InstallScope::Global,
            location: PathBuf::from(loc),
            platforms: vec![platform],
            tracked_for: vec![],
            state: ArtifactState::External,
            version: Some(ver.to_string()),
            source: None,
        };
        let mut art = orphan_artifact("hopper-coordinator");
        art.state = ArtifactState::External;
        art.diverged = true;
        art.version = None;
        art.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = DoctorReport {
            rows: vec![
                mk_row("/u/.claude/skills", "3.3.0", Platform::Claude),
                mk_row("/u/.agents/skills", "3.2.0", Platform::Codex),
            ],
            artifacts: vec![art],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: true,
        };
        let out = r.to_string();
        assert!(out.contains("hopper-coordinator diverges:"), "detail line present: {out}");
        assert!(out.contains("/u/.claude/skills @ 3.3.0"), "claude copy version: {out}");
        assert!(out.contains("/u/.agents/skills @ 3.2.0"), "agents copy version: {out}");
    }

    #[test]
    fn doctor_default_view_surfaces_external_divergence() {
        // A diverged external artifact is an anomaly: the default (problems-only)
        // view must surface it rather than claim everything is healthy.
        let mut a = orphan_artifact("hopper-coordinator");
        a.state = ArtifactState::External;
        a.diverged = true;
        a.version = None;
        a.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = DoctorReport {
            rows: vec![],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
        };
        assert!(r.has_issues(), "a diverged external artifact is an issue");
        let out = r.to_string();
        assert!(out.contains("Needs attention:"), "surfaced in default view: {out}");
        assert!(out.contains("hopper-coordinator"), "the diverged artifact is shown: {out}");
        assert!(
            !out.contains("everything cmx manages is healthy"),
            "must not claim healthy while diverged: {out}"
        );
        assert!(out.contains("1 diverged"), "tally counts it: {out}");
    }

    #[test]
    fn doctor_report_problems_only_by_default() {
        // Default view (show_all=false): a tracked artifact is hidden; only
        // problems surface. With nothing wrong, a healthy message shows.
        let healthy = DoctorReport {
            rows: vec![],
            artifacts: vec![DoctorArtifact {
                kind: ArtifactKind::Skill,
                name: "clipboard".to_string(),
                scope: InstallScope::Global,
                state: ArtifactState::Tracked,
                version: Some("1.0.0".to_string()),
                versions: vec!["1.0.0".to_string()],
                tools: vec![Platform::Claude],
                source: Some("home".to_string()),
                locations: vec![PathBuf::from("/a")],
                diverged: false,
            }],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
        };
        let out = healthy.to_string();
        assert!(!out.contains("clipboard"), "tracked artifact hidden by default: {out}");
        assert!(out.contains("everything cmx manages is healthy"), "healthy message: {out}");
        assert!(out.contains("1 tracked"), "summary still tallies it");
    }
}
