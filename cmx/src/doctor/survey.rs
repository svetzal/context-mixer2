//! Thin orchestrator: wires `locations`/`classify`/`aggregate`/
//! `set_consistency` into the read-only `survey()` entry point.

use crate::config;
use crate::context::AppContext;
use crate::error::Result;
use crate::flags::SurveyScope;
use crate::platform::Platform;

use super::aggregate::{collect_missing, group_rows, sort_missing, sort_rows};
use super::classify::build_rows;
use super::locations::{available_in_sources, build_locations, load_all_locks, survey_scopes};
use super::set_consistency::collect_set_inconsistencies;
use super::types::DoctorReport;

/// Survey the whole system installation and classify every artifact.
///
/// Read-only: performs no writes. Surveys global scope always, and project
/// (local) scope when `scope` includes local.
pub fn survey(scope: SurveyScope, ctx: &AppContext<'_>) -> Result<DoctorReport> {
    let scopes = survey_scopes(scope);
    let cfg = config::load_config(ctx.fs, ctx.paths)?;
    // When the user has declared a managed set, `doctor` surveys only those
    // platforms; otherwise it inspects every supported platform.
    let platforms = if cfg.platforms.is_empty() {
        Platform::ALL.to_vec()
    } else {
        cfg.platforms.clone()
    };
    let locations = build_locations(ctx, &scopes, &platforms)?;
    let locks = load_all_locks(ctx, &scopes, &platforms)?;
    let available = available_in_sources(ctx)?;
    let external = cfg.external;

    let mut rows = build_rows(&locations, &locks, &available, &external, ctx)?;
    let mut missing = collect_missing(&locks, ctx);
    sort_rows(&mut rows);
    sort_missing(&mut missing);
    let artifacts = group_rows(&rows);
    let set_inconsistencies = collect_set_inconsistencies(&scopes, &artifacts, ctx)?;

    Ok(DoctorReport {
        rows,
        artifacts,
        missing,
        included_local: scope.includes_local(),
        surveyed_platforms: platforms.len(),
        scoped_to_managed: !cfg.platforms.is_empty(),
        show_all: false,
        set_inconsistencies,
    })
}
