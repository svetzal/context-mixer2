use std::fmt;

use serde_json::{Value, json};

use crate::doctor::{DoctorArtifact, DoctorReport};
use crate::table::Table;

/// The artifacts this survey shows — the same selection whether rendered as
/// the human table or projected to JSON. By default `doctor` shows only what
/// needs attention (it's a doctor); `--all` shows the full inventory.
fn shown_artifacts(report: &DoctorReport) -> Vec<&DoctorArtifact> {
    if report.show_all {
        report.artifacts.iter().collect()
    } else {
        report.artifacts.iter().filter(|a| DoctorReport::is_problem(a)).collect()
    }
}

/// Message printed to stderr when `--adopt-all` is used: it is deprecated and
/// will be removed in the next major release; the canonical replacement is the
/// first-class `adopt` subcommand on each artifact kind.
pub fn adopt_all_deprecation_notice() -> &'static str {
    "warning: `cmx doctor --adopt-all` is deprecated and will be removed in the \
     next major release. Use `cmx skill adopt --all` / `cmx agent adopt --all` \
     instead (pass `--from-dir <dir>` there in place of doctor's `--from`)."
}

/// Project the survey to the machine-readable schema documented for
/// `cmx doctor --json`. Mirrors the human `Display` impl's content and
/// selection (via [`shown_artifacts`]), but structures divergence as a
/// `locations` array instead of free-text prose.
pub fn doctor_json(report: &DoctorReport) -> Value {
    let shown = shown_artifacts(report);
    let c = report.counts();
    let artifacts: Vec<Value> = shown
        .iter()
        .map(|a| {
            let mut obj = json!({
                "kind": a.kind.to_string(),
                "name": a.name.clone(),
                "scope": a.scope.label(),
                "state": a.state.label(),
                "source": a.source.clone(),
                "tools": a.tools.iter().map(ToString::to_string).collect::<Vec<_>>(),
                "diverged": a.diverged,
                "locations": crate::doctor::location_members(a, &report.rows)
                    .into_iter()
                    .map(|m| json!({
                        "path": m.location.display().to_string(),
                        "platform": m.platform.map(|p| p.to_string()),
                        "version": m.version,
                        "state": m.state_label,
                    }))
                    .collect::<Vec<_>>(),
            });
            let map = obj.as_object_mut().expect("json! object literal");
            if let Some(version) = &a.version {
                map.insert("version".to_string(), json!(version));
            } else {
                map.insert("versions".to_string(), json!(a.versions));
            }
            obj
        })
        .collect();

    json!({
        "scope": if report.included_local { "global+local" } else { "global" },
        "platforms_surveyed": report.surveyed_platforms,
        "showing": if report.show_all { "all" } else { "needs_attention" },
        "summary": {
            "tracked": c.tracked,
            "drifted": c.drifted,
            "untracked": c.untracked,
            "orphaned": c.orphaned,
            "external": c.external,
            "missing": c.missing,
            "diverged": c.diverged,
            "set_inconsistent": c.set_inconsistent,
        },
        "artifacts": artifacts,
        "set_inconsistencies": report
            .set_inconsistencies
            .iter()
            .map(|s| json!({
                "set": s.set_name,
                "scope": s.scope.label(),
                "kind": s.kind.to_string(),
                "member": s.member,
                "problem": set_problem_label(s.problem),
            }))
            .collect::<Vec<_>>(),
    })
}

/// Machine-readable label for a [`crate::doctor::SetProblem`] — used by both
/// `doctor_json` and the human hint text.
fn set_problem_label(problem: crate::doctor::SetProblem) -> &'static str {
    match problem {
        crate::doctor::SetProblem::ActiveMissing => "active_missing",
        crate::doctor::SetProblem::InactiveLingering => "inactive_lingering",
    }
}

fn platform_version_label(
    platform: Option<crate::platform::Platform>,
    version: Option<&str>,
) -> String {
    let platform = platform.map_or_else(|| "unmapped".to_string(), |p| p.to_string());
    let version = version.unwrap_or("unversioned");
    format!("{platform}@{version}")
}

fn doctor_platforms_cell(
    artifact: &crate::doctor::DoctorArtifact,
    rows: &[crate::doctor::DoctorRow],
) -> String {
    let members = crate::doctor::location_members(artifact, rows);
    let labels: Vec<String> = if members.is_empty() {
        artifact
            .tools
            .iter()
            .copied()
            .map(|platform| platform_version_label(Some(platform), artifact.version.as_deref()))
            .collect()
    } else {
        members
            .into_iter()
            .flat_map(|member| {
                if member.platforms.is_empty() {
                    vec![platform_version_label(None, member.version.as_deref())]
                } else {
                    member
                        .platforms
                        .into_iter()
                        .map(|platform| {
                            platform_version_label(Some(platform), member.version.as_deref())
                        })
                        .collect()
                }
            })
            .collect()
    };

    if labels.is_empty() {
        platform_version_label(None, artifact.version.as_deref())
    } else {
        labels.join(", ")
    }
}

/// Build the artifact table from the given grouped logical artifacts — one row
/// per logical artifact, with the Platforms column attributing versions to the
/// surveyed install locations.
fn doctor_artifact_table(
    artifacts: &[&crate::doctor::DoctorArtifact],
    rows: &[crate::doctor::DoctorRow],
) -> Table {
    Table {
        headers: vec!["Type", "Name", "Scope", "State", "Source", "Platforms"],
        padded_cols: 6,
        rows: artifacts
            .iter()
            .map(|a| {
                let mut cells = vec![
                    a.kind.to_string(),
                    a.name.clone(),
                    a.scope.label().to_string(),
                    a.state.label().to_string(),
                    a.source.clone().unwrap_or_else(|| "none".to_string()),
                    doctor_platforms_cell(a, rows),
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
            "  • {} orphaned artifact(s) have no source (hand-authored) — `cmx <kind> adopt <name>` (or `cmx <kind> adopt --all`) canonicalizes them into the home.",
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
            "  • {} artifact(s) diverge across their install locations (their content differs). Pick by your situation:\n\
             \x20   - source- or home-backed, edited in place → `cmx skill promote <name>` (shows a plan; add `--apply`)\n\
             \x20   - source-backed, restore from source      → `cmx skill update <name> --force`\n\
             \x20   - external / source-less                  → `cmx skill sync <name>` (or `--from <platform>`; add `--apply`)\n\
             \x20   - not sure? inspect first                  → `cmx skill diff <name>`",
            c.diverged
        ));
    }
    if c.set_inconsistent > 0 {
        lines.push(format!(
            "  • {} set/installed mismatch(es) — an active set is missing a member, or an inactive \
             set's member is still installed on its behalf. `cmx set activate <name>` repairs a \
             missing member after showing its plan; `cmx set deactivate <name>` previews first \
             and clears a lingering one with `--apply`.",
            c.set_inconsistent
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

/// Detail lines for each set/installed-state mismatch, naming the set, the
/// member, and what's wrong — one line per [`crate::doctor::SetInconsistency`].
fn doctor_set_details(report: &DoctorReport) -> String {
    if report.set_inconsistencies.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    for s in &report.set_inconsistencies {
        let what = match s.problem {
            crate::doctor::SetProblem::ActiveMissing => "active but not installed",
            crate::doctor::SetProblem::InactiveLingering => {
                "inactive but still installed (not held by any active set)"
            }
        };
        lines.push(format!(
            "  • set '{}' ({}): {} {} is {what}",
            s.set_name,
            s.scope.label(),
            s.kind,
            s.member
        ));
    }
    format!("\n{}\n", lines.join("\n"))
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
        // `--all` shows the full inventory. Same selection `doctor_json` uses.
        let shown = shown_artifacts(self);

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
            write!(f, "{}", doctor_artifact_table(&shown, &self.rows).render())?;
        }

        if !self.missing.is_empty() {
            if !shown.is_empty() {
                writeln!(f)?;
            }
            writeln!(f, "Missing (in a lock file, absent on disk):")?;
            write!(f, "{}", doctor_missing_table(self).render())?;
        }

        if shown.is_empty() && self.missing.is_empty() && self.set_inconsistencies.is_empty() {
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
            "\nSummary: {} tracked, {} drifted, {} untracked, {} orphaned, {} external, {} missing · {} diverged · {} set-inconsistent.",
            c.tracked,
            c.drifted,
            c.untracked,
            c.orphaned,
            c.external,
            c.missing,
            c.diverged,
            c.set_inconsistent
        )?;
        write!(f, "{}", doctor_hints(&c))?;
        write!(f, "{}", doctor_divergence_details(&shown, &self.rows))?;
        write!(f, "{}", doctor_set_details(self))
    }
}

#[cfg(test)]
mod tests {
    use super::{adopt_all_deprecation_notice, doctor_json};
    use crate::doctor::{
        ArtifactState, DoctorArtifact, DoctorReport, DoctorRow, MissingRow, SetInconsistency,
        SetProblem,
    };
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

    fn skill_row(
        name: &str,
        location: &str,
        platforms: Vec<Platform>,
        state: ArtifactState,
        version: Option<&str>,
    ) -> DoctorRow {
        DoctorRow {
            kind: ArtifactKind::Skill,
            name: name.to_string(),
            scope: InstallScope::Global,
            location: PathBuf::from(location),
            platforms,
            tracked_for: vec![],
            state,
            version: version.map(str::to_string),
            source: None,
            content_checksum: format!("sha256:{name}:{location}:{}", version.unwrap_or("none")),
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
            set_inconsistencies: vec![],
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
    fn doctor_report_lists_platform_versions_for_multi_platform_artifact() {
        // One skill installed for two platforms is ONE row listing both copies
        // as platform@version pairs — not one row per location.
        let r = DoctorReport {
            rows: vec![skill_row(
                "clipboard",
                "/shared/.agents/skills",
                vec![Platform::Claude, Platform::Codex],
                ArtifactState::Tracked,
                Some("1.0.0"),
            )],
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
            set_inconsistencies: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("clipboard"));
        assert!(
            out.contains("claude@1.0.0, codex@1.0.0"),
            "platform/version pairs listed in one row: {out}"
        );
        assert!(out.contains("Platforms"), "doctor header renamed: {out}");
        assert!(!out.contains("Tools"), "stale header removed: {out}");
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
            set_inconsistencies: vec![],
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
            set_inconsistencies: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("(diverged)"), "diverged marker rendered: {out}");
        assert!(out.contains("1 diverged"));
        assert!(out.contains("diverge across their install locations"), "diverged hint present");
    }

    #[test]
    fn doctor_report_names_version_skew() {
        // A version-diverged artifact attributes each version to its platform.
        let mut a = orphan_artifact("hopper-coordinator");
        a.diverged = true;
        a.version = None;
        a.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = DoctorReport {
            rows: vec![
                skill_row(
                    "hopper-coordinator",
                    "/u/.agents/skills",
                    vec![Platform::Codex],
                    ArtifactState::External,
                    Some("3.2.0"),
                ),
                skill_row(
                    "hopper-coordinator",
                    "/u/.claude/skills",
                    vec![Platform::Claude],
                    ArtifactState::External,
                    Some("3.3.0"),
                ),
            ],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: true,
            set_inconsistencies: vec![],
        };
        let out = r.to_string();
        assert!(out.contains("codex@3.2.0, claude@3.3.0"), "skew attributed in the table: {out}");
        assert!(out.contains("none"), "source uses explicit placeholder: {out}");
        assert!(out.contains("(diverged)"), "still flagged diverged: {out}");
    }

    #[test]
    fn doctor_details_name_each_locations_version() {
        let mut art = orphan_artifact("hopper-coordinator");
        art.state = ArtifactState::External;
        art.diverged = true;
        art.version = None;
        art.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        let r = DoctorReport {
            rows: vec![
                skill_row(
                    "hopper-coordinator",
                    "/u/.claude/skills",
                    vec![Platform::Claude],
                    ArtifactState::External,
                    Some("3.3.0"),
                ),
                skill_row(
                    "hopper-coordinator",
                    "/u/.agents/skills",
                    vec![Platform::Codex],
                    ArtifactState::External,
                    Some("3.2.0"),
                ),
            ],
            artifacts: vec![art],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: true,
            set_inconsistencies: vec![],
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
            rows: vec![
                skill_row(
                    "hopper-coordinator",
                    "/u/.agents/skills",
                    vec![Platform::Codex],
                    ArtifactState::External,
                    Some("3.2.0"),
                ),
                skill_row(
                    "hopper-coordinator",
                    "/u/.claude/skills",
                    vec![Platform::Claude],
                    ArtifactState::External,
                    Some("3.3.0"),
                ),
            ],
            artifacts: vec![a],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
            set_inconsistencies: vec![],
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
            set_inconsistencies: vec![],
        };
        let out = healthy.to_string();
        assert!(!out.contains("clipboard"), "tracked artifact hidden by default: {out}");
        assert!(out.contains("everything cmx manages is healthy"), "healthy message: {out}");
        assert!(out.contains("1 tracked"), "summary still tallies it");
    }

    fn diverged_report_with_locations() -> DoctorReport {
        let mut art = orphan_artifact("hopper-coordinator");
        art.state = ArtifactState::External;
        art.diverged = true;
        art.version = None;
        art.versions = vec!["3.2.0".to_string(), "3.3.0".to_string()];
        art.locations = vec![
            PathBuf::from("/u/.claude/skills"),
            PathBuf::from("/u/.agents/skills"),
        ];
        DoctorReport {
            rows: vec![
                skill_row(
                    "hopper-coordinator",
                    "/u/.claude/skills",
                    vec![Platform::Claude],
                    ArtifactState::External,
                    Some("3.3.0"),
                ),
                skill_row(
                    "hopper-coordinator",
                    "/u/.agents/skills",
                    vec![Platform::Codex],
                    ArtifactState::External,
                    Some("3.2.0"),
                ),
            ],
            artifacts: vec![art],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
            set_inconsistencies: vec![],
        }
    }

    #[test]
    fn doctor_json_schema_pins_shape() {
        let r = diverged_report_with_locations();
        let value = doctor_json(&r);

        assert_eq!(value["scope"], "global");
        assert_eq!(value["platforms_surveyed"], 13);
        assert_eq!(value["showing"], "needs_attention");

        let summary = &value["summary"];
        for key in [
            "tracked",
            "drifted",
            "untracked",
            "orphaned",
            "external",
            "missing",
            "diverged",
            "set_inconsistent",
        ] {
            assert!(summary.get(key).is_some(), "summary missing {key}: {value}");
        }
        assert_eq!(summary["diverged"], 1);
        assert_eq!(summary["set_inconsistent"], 0);
        assert!(value["set_inconsistencies"].as_array().expect("array").is_empty());

        let artifacts = value["artifacts"].as_array().expect("artifacts array");
        assert_eq!(artifacts.len(), 1);
        let a = &artifacts[0];
        assert_eq!(a["name"], "hopper-coordinator");
        assert_eq!(a["diverged"], true);
        assert_eq!(a["versions"], serde_json::json!(["3.2.0", "3.3.0"]));

        let locations = a["locations"].as_array().expect("locations array");
        assert_eq!(locations.len(), 2);
        for loc in locations {
            assert!(loc.get("path").is_some());
            assert!(loc.get("platform").is_some());
            assert!(loc.get("version").is_some());
            assert!(loc.get("state").is_some());
        }
        assert_eq!(locations[0]["platform"], "codex");
        assert_eq!(locations[1]["platform"], "claude");
    }

    #[test]
    fn doctor_json_showing_all() {
        let mut r = diverged_report_with_locations();
        r.rows.push(skill_row(
            "clipboard",
            "/u/.claude/skills",
            vec![Platform::Claude],
            ArtifactState::Tracked,
            Some("1.0.0"),
        ));
        r.artifacts.push(DoctorArtifact {
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
        });
        r.show_all = true;

        let value = doctor_json(&r);
        assert_eq!(value["showing"], "all");
        let artifacts = value["artifacts"].as_array().expect("artifacts array");
        assert_eq!(artifacts.len(), 2, "healthy tracked artifact included with --all: {value}");
        assert!(artifacts.iter().any(|a| a["name"] == "clipboard"));
    }

    #[test]
    fn doctor_report_renders_external_platform_versions_without_dash_cells() {
        let report = diverged_report_with_locations();
        let out = report.to_string();
        assert!(out.contains("codex@3.2.0, claude@3.3.0"), "external copies attributed: {out}");
        assert!(out.contains("none"), "source is explicit, not a dash: {out}");
        assert!(
            out.contains("external  none    codex@3.2.0, claude@3.3.0"),
            "table row no longer has dash placeholders: {out}"
        );
    }

    #[test]
    fn doctor_report_renders_unversioned_platform_cell() {
        let report = DoctorReport {
            rows: vec![skill_row(
                "personal-finance",
                "/u/.claude/skills",
                vec![Platform::Claude],
                ArtifactState::Drifted,
                None,
            )],
            artifacts: vec![DoctorArtifact {
                kind: ArtifactKind::Skill,
                name: "personal-finance".to_string(),
                scope: InstallScope::Global,
                state: ArtifactState::Drifted,
                version: None,
                versions: vec![],
                tools: vec![Platform::Claude],
                source: Some("home".to_string()),
                locations: vec![PathBuf::from("/u/.claude/skills")],
                diverged: false,
            }],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: true,
            set_inconsistencies: vec![],
        };
        let out = report.to_string();
        assert!(out.contains("claude@unversioned"), "unversioned vocabulary preserved: {out}");
        assert!(!out.contains("Tools"), "doctor output no longer mentions Tools: {out}");
    }

    #[test]
    fn doctor_report_flags_set_inconsistency() {
        let r = DoctorReport {
            rows: vec![],
            artifacts: vec![],
            missing: vec![],
            included_local: false,
            surveyed_platforms: 13,
            scoped_to_managed: false,
            show_all: false,
            set_inconsistencies: vec![SetInconsistency {
                set_name: "rust-work".to_string(),
                scope: InstallScope::Global,
                kind: ArtifactKind::Agent,
                member: "rust-craftsperson".to_string(),
                problem: SetProblem::ActiveMissing,
            }],
        };
        assert!(r.has_issues(), "a set inconsistency is an issue");
        let out = r.to_string();
        assert!(
            !out.contains("everything cmx manages is healthy"),
            "must not claim healthy with a set inconsistency: {out}"
        );
        assert!(out.contains("1 set-inconsistent"), "summary tallies it: {out}");
        assert!(out.contains("cmx set activate"), "hint present: {out}");
        assert!(
            out.contains("rust-work") && out.contains("rust-craftsperson"),
            "detail line names the set and member: {out}"
        );

        let value = doctor_json(&r);
        assert_eq!(value["summary"]["set_inconsistent"], 1);
        let entries = value["set_inconsistencies"].as_array().expect("array");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["set"], "rust-work");
        assert_eq!(entries[0]["problem"], "active_missing");
    }

    #[test]
    fn adopt_all_deprecation_notice_steers_to_canonical() {
        let notice = adopt_all_deprecation_notice();
        assert!(notice.contains("deprecated"));
        assert!(notice.contains("next major"));
        assert!(notice.contains("cmx skill adopt --all"));
        assert!(notice.contains("cmx agent adopt --all"));
    }
}
