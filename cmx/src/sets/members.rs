//! Set membership management.

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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use crate::error::CliError;
    use crate::test_support::{
        TestContext, install_skill_on_disk, make_lock_entry_builder, save_lock_with_entry,
        setup_empty_sources, skill_content,
    };
    use crate::types::{ArtifactKind, InstallScope, SetMember};

    use super::{member_description_chars, parse_prefix, resolve_member, seed_from_plugin};

    // -----------------------------------------------------------------------
    // parse_prefix
    // -----------------------------------------------------------------------

    #[test]
    fn parse_prefix_skill_colon_strips_prefix() {
        let (kind, name) = parse_prefix("skill:my-skill");
        assert_eq!(kind, Some(ArtifactKind::Skill));
        assert_eq!(name, "my-skill");
    }

    #[test]
    fn parse_prefix_agent_colon_strips_prefix() {
        let (kind, name) = parse_prefix("agent:my-agent");
        assert_eq!(kind, Some(ArtifactKind::Agent));
        assert_eq!(name, "my-agent");
    }

    #[test]
    fn parse_prefix_bare_name_returns_none_kind() {
        let (kind, name) = parse_prefix("my-skill");
        assert!(kind.is_none());
        assert_eq!(name, "my-skill");
    }

    #[test]
    fn parse_prefix_colon_in_name_not_prefixed_is_left_intact() {
        // A name like "my:thing" should not be mis-split
        let (kind, name) = parse_prefix("my:thing");
        // Neither "my" nor "thing" is a recognized kind prefix
        assert!(kind.is_none(), "unrecognized prefix must not be parsed as a kind");
        assert_eq!(name, "my:thing", "unrecognized colon-prefix must be left intact");
    }

    // -----------------------------------------------------------------------
    // resolve_member — error branches
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_member_errors_when_name_not_in_lockfile() {
        let t = TestContext::new();
        setup_empty_sources(&t.fs, &t.paths);
        let ctx = t.ctx();
        let err = resolve_member("no-such-skill", &ctx).unwrap_err();
        assert!(
            matches!(err, CliError::ArtifactNotInLockfile { .. }),
            "missing artifact must produce ArtifactNotInLockfile, got {err:?}"
        );
    }

    #[test]
    fn resolve_member_errors_when_hint_does_not_match_kind() {
        // Install a skill but ask for it as an agent
        let t = TestContext::new();
        let entry = make_lock_entry_builder(ArtifactKind::Skill, "test-source", "skills/alpha");
        save_lock_with_entry(&t.fs, &t.paths, "alpha", entry, InstallScope::Global);
        setup_empty_sources(&t.fs, &t.paths);
        let ctx = t.ctx();
        // "agent:alpha" asks for the agent kind, but only a skill is in the lock
        let err = resolve_member("agent:alpha", &ctx).unwrap_err();
        assert!(
            matches!(err, CliError::ArtifactNoMatchingKind { .. }),
            "kind hint mismatch must produce ArtifactNoMatchingKind, got {err:?}"
        );
    }

    #[test]
    fn resolve_member_errors_when_same_name_both_kinds_no_hint() {
        // Both an agent and a skill named "alpha" exist in the lock
        let t = TestContext::new();
        let skill_entry =
            make_lock_entry_builder(ArtifactKind::Skill, "test-source", "skills/alpha");
        save_lock_with_entry(&t.fs, &t.paths, "alpha", skill_entry, InstallScope::Global);

        // Save the agent entry in the local scope lock so the global scope isn't replaced
        let agent_entry =
            make_lock_entry_builder(ArtifactKind::Agent, "test-source", "agents/alpha.md");
        save_lock_with_entry(&t.fs, &t.paths, "alpha", agent_entry, InstallScope::Local);
        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let err = resolve_member("alpha", &ctx).unwrap_err();
        assert!(
            matches!(err, CliError::ArtifactAmbiguousKind { .. }),
            "ambiguous name must produce ArtifactAmbiguousKind, got {err:?}"
        );
    }

    #[test]
    fn resolve_member_disambiguates_with_skill_hint() {
        let t = TestContext::new();
        // Same-name entries in different scopes (to get both kinds into candidates)
        let skill_entry =
            make_lock_entry_builder(ArtifactKind::Skill, "test-source", "skills/alpha");
        save_lock_with_entry(&t.fs, &t.paths, "alpha", skill_entry, InstallScope::Global);
        let agent_entry =
            make_lock_entry_builder(ArtifactKind::Agent, "test-source", "agents/alpha.md");
        save_lock_with_entry(&t.fs, &t.paths, "alpha", agent_entry, InstallScope::Local);
        setup_empty_sources(&t.fs, &t.paths);

        let ctx = t.ctx();
        let member = resolve_member("skill:alpha", &ctx).unwrap();
        assert_eq!(member.kind, ArtifactKind::Skill);
        assert_eq!(member.name, "alpha");
        assert_eq!(member.source.as_deref(), Some("test-source"));
    }

    // -----------------------------------------------------------------------
    // seed_from_plugin — malformed-spec error branches
    // -----------------------------------------------------------------------

    #[test]
    fn seed_from_plugin_errors_when_spec_has_no_colon() {
        let t = TestContext::new();
        setup_empty_sources(&t.fs, &t.paths);
        let ctx = t.ctx();
        let err = seed_from_plugin("no-colon", &ctx).unwrap_err();
        assert!(
            matches!(err, CliError::InvalidFromPlugin { .. }),
            "missing colon must produce InvalidFromPlugin, got {err:?}"
        );
    }

    #[test]
    fn seed_from_plugin_errors_when_source_is_empty() {
        let t = TestContext::new();
        setup_empty_sources(&t.fs, &t.paths);
        let ctx = t.ctx();
        let err = seed_from_plugin(":plugin", &ctx).unwrap_err();
        assert!(
            matches!(err, CliError::InvalidFromPlugin { .. }),
            "empty source must produce InvalidFromPlugin, got {err:?}"
        );
    }

    #[test]
    fn seed_from_plugin_errors_when_plugin_is_empty() {
        let t = TestContext::new();
        setup_empty_sources(&t.fs, &t.paths);
        let ctx = t.ctx();
        let err = seed_from_plugin("source:", &ctx).unwrap_err();
        assert!(
            matches!(err, CliError::InvalidFromPlugin { .. }),
            "empty plugin must produce InvalidFromPlugin, got {err:?}"
        );
    }

    // -----------------------------------------------------------------------
    // member_description_chars
    // -----------------------------------------------------------------------

    #[test]
    fn member_description_chars_returns_installed_description_when_present() {
        let t = TestContext::new();
        setup_empty_sources(&t.fs, &t.paths);
        let content = skill_content("A short description");
        install_skill_on_disk(&t.fs, &t.paths, "alpha", &content, InstallScope::Global);
        let ctx = t.ctx();
        let member = SetMember {
            kind: ArtifactKind::Skill,
            name: "alpha".to_string(),
            source: None,
        };
        let chars = member_description_chars(&member, &ctx);
        assert_eq!(chars, Some("A short description".chars().count()));
    }

    #[test]
    fn member_description_chars_returns_none_when_not_installed_and_no_source() {
        let t = TestContext::new();
        setup_empty_sources(&t.fs, &t.paths);
        let ctx = t.ctx();
        let member = SetMember {
            kind: ArtifactKind::Skill,
            name: "not-installed".to_string(),
            source: None,
        };
        let chars = member_description_chars(&member, &ctx);
        assert!(chars.is_none(), "unresolvable member must return None");
    }
}
