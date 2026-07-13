use crate::error::{CliError, Result};
use std::collections::HashSet;

use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::scan_marketplace::{PluginScan, scan_marketplace_plugin};
use crate::source_iter;
use crate::types::{ArtifactKind, InstallScope, SetMember};

/// Resolve a `<source>:<plugin>` spec to the plugin's declared `agents`/
/// `skills`, pinning each resulting member's `source` to the given source
/// name (see `SETS.md`, "Relationship to the existing 'plugin' concept").
/// Unlike [`resolve_member`], which resolves from the *lockfile* (the
/// artifact must already be installed), this resolves from the *source's*
/// marketplace — the whole point of seeding is that members need not be
/// installed yet.
pub(super) fn seed_from_plugin(spec: &str, ctx: &AppContext<'_>) -> Result<Vec<SetMember>> {
    let Some((source_name, plugin_name)) = spec.split_once(':') else {
        return Err(CliError::InvalidFromPlugin {
            spec: spec.to_string(),
        });
    };
    if source_name.is_empty() || plugin_name.is_empty() {
        return Err(CliError::InvalidFromPlugin {
            spec: spec.to_string(),
        });
    }

    let sources = config::load_sources(ctx.fs, ctx.paths)?;
    let entry = sources.get_source(source_name)?;
    let root = config::resolve_local_path(entry)?;

    let marketplace_path = root.join(".claude-plugin").join("marketplace.json");
    if !ctx.fs.exists(&marketplace_path) {
        return Err(CliError::SourceNoMarketplace {
            source_name: source_name.to_string(),
        });
    }

    let mut warnings = Vec::new();
    let scan =
        scan_marketplace_plugin(&root, &marketplace_path, plugin_name, ctx.fs, &mut warnings)?;

    match scan {
        PluginScan::NotFound => Err(CliError::PluginNotFound {
            plugin: plugin_name.to_string(),
            source_name: source_name.to_string(),
        }),
        PluginScan::RemoteUnsupported(source_type) => Err(CliError::PluginRemoteUnsupported {
            plugin: plugin_name.to_string(),
            source_type,
        }),
        PluginScan::Found(artifacts) => Ok(artifacts
            .into_iter()
            .map(|a| SetMember {
                kind: a.kind,
                name: a.name,
                source: Some(source_name.to_string()),
            })
            .collect()),
    }
}

/// Split an optional `skill:`/`agent:` disambiguation prefix off an artifact
/// argument. `source:name` disambiguation is deferred (SETS.md Phase 4).
pub(super) fn parse_prefix(arg: &str) -> (Option<ArtifactKind>, &str) {
    if let Some(rest) = arg.strip_prefix("skill:") {
        (Some(ArtifactKind::Skill), rest)
    } else if let Some(rest) = arg.strip_prefix("agent:") {
        (Some(ArtifactKind::Agent), rest)
    } else {
        (None, arg)
    }
}

/// Resolve an artifact argument to its kind and pinned source by consulting
/// the lockfile (see `SETS.md`, "The source pin"). Searches both install
/// scopes; a bare name that is ambiguous across kinds requires a `skill:`/
/// `agent:` prefix to disambiguate.
pub(super) fn resolve_member(arg: &str, ctx: &AppContext<'_>) -> Result<SetMember> {
    let (hint, name) = parse_prefix(arg);

    let mut candidates: Vec<(ArtifactKind, String)> = Vec::new();
    for scope in InstallScope::ALL {
        let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
        if let Some(entry) = lock.packages.get(name) {
            candidates.push((entry.artifact_type, entry.source.repo.clone()));
        }
    }

    if candidates.is_empty() {
        return Err(CliError::ArtifactNotInLockfile {
            name: name.to_string(),
            hint: crate::suggestions::installed_artifact_hint(name, hint, ctx),
        });
    }

    let candidates: Vec<(ArtifactKind, String)> = match hint {
        Some(k) => candidates.into_iter().filter(|(kind, _)| *kind == k).collect(),
        None => candidates,
    };

    if candidates.is_empty() {
        return Err(CliError::ArtifactNoMatchingKind {
            name: name.to_string(),
        });
    }

    let distinct_kinds: HashSet<ArtifactKind> = candidates.iter().map(|(k, _)| *k).collect();
    if hint.is_none() && distinct_kinds.len() > 1 {
        return Err(CliError::ArtifactAmbiguousKind {
            name: name.to_string(),
        });
    }

    let (kind, source) = candidates.into_iter().next().expect("non-empty, checked above");
    Ok(SetMember {
        kind,
        name: name.to_string(),
        source: Some(source),
    })
}

/// Resolve a member's trigger-description character count (see `SETS.md`,
/// "Context-footprint reporting"): read the installed copy's `description`
/// frontmatter when the member happens to be installed (either scope), else
/// fall back to its pinned source. `None` when neither yields a description —
/// callers treat that as an unresolvable member, counted as 0 in totals and
/// rendered as `?`.
pub(super) fn member_description_chars(m: &SetMember, ctx: &AppContext<'_>) -> Option<usize> {
    InstallScope::ALL
        .iter()
        .find_map(|&scope| installed_description_chars(m, scope, ctx))
        .or_else(|| source_description_chars(m, ctx))
}

pub(super) fn installed_description_chars(
    m: &SetMember,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Option<usize> {
    let path = ctx.paths.installed_artifact_path(m.kind, &m.name, scope)?;
    if !ctx.fs.exists(&path) {
        return None;
    }
    let content = ctx.fs.read_to_string(&m.kind.content_path(&path)).ok()?;
    let (frontmatter, _) = crate::scan::split_frontmatter_and_body(&content);
    crate::scan::extract_field(&frontmatter?, "description").map(|d| d.chars().count())
}

pub(super) fn source_description_chars(m: &SetMember, ctx: &AppContext<'_>) -> Option<usize> {
    source_iter::find_unique(&m.name, m.kind, m.source.as_deref(), ctx)
        .ok()
        .map(|sa| sa.artifact.description.chars().count())
}
