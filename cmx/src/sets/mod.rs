//! `cmx set` — definitions, curation, and the activation lifecycle for sets.
//!
//! A set is a locally-defined, named group of installed artifacts with a
//! desired activation state (see `SETS.md`). `create`, `list`, `show`, `add`,
//! `remove`, `rename` are pure curation with no install/uninstall side
//! effects. `activate`/`deactivate` (Phase 2) compose the existing
//! `install`/`uninstall` machinery to make that state actionable; `delete
//! --purge` deactivates first. Context-footprint reporting and `doctor`
//! integration are Phase 3.

mod types;
pub use types::*;

mod members;
use members::{member_description_chars, parse_prefix, resolve_member, seed_from_plugin};

mod activation;
pub use activation::{activate, deactivate};

use crate::error::{CliError, Result};
use crate::flags::{Force, Purge, RunMode};

use crate::config;
use crate::context::AppContext;
use crate::types::{ArtifactKind, InstallScope, SetDef, SetMember, SetState};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn create(
    name: &str,
    description: Option<&str>,
    from: Option<&str>,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetCreateResult> {
    // Resolve-then-mutate: fail fast, before writing anything, if `--from-plugin`
    // can't be resolved to a known source/plugin.
    let members = match from {
        Some(spec) => seed_from_plugin(spec, ctx)?,
        None => Vec::new(),
    };
    let member_count = members.len();

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
        if sets.sets.contains_key(name) {
            return Err(CliError::SetAlreadyExists {
                name: name.to_string(),
            });
        }
        sets.sets.insert(
            name.to_string(),
            SetDef {
                description: description.map(str::to_string),
                state: SetState::Inactive,
                members,
            },
        );
        Ok(())
    })?;
    Ok(SetCreateResult {
        name: name.to_string(),
        member_count,
        seeded_from: from.map(str::to_string),
    })
}

pub fn list(scope: InstallScope, ctx: &AppContext<'_>) -> Result<SetListResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let entries = sets
        .sets
        .into_iter()
        .map(|(name, def)| {
            let footprint_chars =
                def.members.iter().map(|m| member_description_chars(m, ctx).unwrap_or(0)).sum();
            SetListEntry {
                name,
                state: def.state,
                member_count: def.members.len(),
                footprint_chars,
            }
        })
        .collect();
    Ok(SetListResult { entries })
}

pub fn show(name: &str, scope: InstallScope, ctx: &AppContext<'_>) -> Result<SetShowResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let def = sets.sets.get(name).ok_or_else(|| CliError::SetNotFound {
        name: name.to_string(),
    })?;

    let members: Vec<SetMemberStatus> = def
        .members
        .iter()
        .map(|m| {
            let installed = InstallScope::ALL
                .iter()
                .any(|&s| ctx.paths.is_installed(m.kind, &m.name, s, ctx.fs));
            SetMemberStatus {
                kind: m.kind,
                name: m.name.clone(),
                source: m.source.clone(),
                installed,
                footprint_chars: member_description_chars(m, ctx),
            }
        })
        .collect();
    let footprint_chars = members.iter().filter_map(|m| m.footprint_chars).sum();

    Ok(SetShowResult {
        name: name.to_string(),
        description: def.description.clone(),
        state: def.state,
        members,
        footprint_chars,
    })
}

pub fn add(
    name: &str,
    artifacts: &[String],
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetAddResult> {
    // Resolve-then-mutate: fail fast, before writing anything, if any artifact
    // cannot be resolved to a kind + source.
    let resolved: Vec<SetMember> =
        artifacts.iter().map(|arg| resolve_member(arg, ctx)).collect::<Result<_>>()?;

    let mut added = Vec::new();
    let mut already = Vec::new();

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
        let def = sets.sets.get_mut(name).ok_or_else(|| CliError::SetNotFound {
            name: name.to_string(),
        })?;

        for member in resolved {
            let is_duplicate = def
                .members
                .iter()
                .any(|existing| existing.kind == member.kind && existing.name == member.name);
            if is_duplicate {
                already.push(member.name);
            } else {
                added.push(member.name.clone());
                def.members.push(member);
            }
        }
        Ok(())
    })?;

    Ok(SetAddResult {
        set: name.to_string(),
        added,
        already,
    })
}

pub fn remove(
    name: &str,
    artifacts: &[String],
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetRemoveResult> {
    let parsed: Vec<(Option<ArtifactKind>, &str)> =
        artifacts.iter().map(|a| parse_prefix(a)).collect();

    let mut removed = Vec::new();
    let mut not_found = Vec::new();

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
        let def = sets.sets.get_mut(name).ok_or_else(|| CliError::SetNotFound {
            name: name.to_string(),
        })?;

        for (hint, artifact_name) in &parsed {
            let before = def.members.len();
            def.members.retain(|m| {
                let matches = m.name == *artifact_name && hint.is_none_or(|k| m.kind == k);
                !matches
            });
            if def.members.len() < before {
                removed.push((*artifact_name).to_string());
            } else {
                not_found.push((*artifact_name).to_string());
            }
        }
        Ok(())
    })?;

    Ok(SetRemoveResult {
        set: name.to_string(),
        removed,
        not_found,
    })
}

/// Delete a set's definition. With [`Purge::Yes`], deactivate it first
/// (honouring reference-counting and the drift guard — `force` forwards to
/// that deactivation) so members not held by another active set are uninstalled
/// before the definition disappears. A drift-blocked member aborts the purge
/// entirely, leaving the definition (and set state) untouched so the user can
/// retry with `--force` or resolve the edits manually.
pub fn delete(
    name: &str,
    purge: Purge,
    force: Force,
    mode: RunMode,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetDeleteResult> {
    if purge.is_yes() {
        let outcome = deactivate(name, force, mode, scope, ctx)?;
        let deleted = if mode.is_apply() && !outcome.any_blocked {
            config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
                sets.sets.remove(name).ok_or_else(|| CliError::SetNotFound {
                    name: name.to_string(),
                })?;
                Ok(())
            })?;
            true
        } else {
            false
        };
        return Ok(SetDeleteResult {
            name: name.to_string(),
            purge: true,
            apply: mode.is_apply(),
            deleted,
            deactivate: Some(outcome),
        });
    }

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
        sets.sets.remove(name).ok_or_else(|| CliError::SetNotFound {
            name: name.to_string(),
        })?;
        Ok(())
    })?;
    Ok(SetDeleteResult {
        name: name.to_string(),
        purge: false,
        apply: true,
        deleted: true,
        deactivate: None,
    })
}

pub fn rename(
    old: &str,
    new: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetRenameResult> {
    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
        if sets.sets.contains_key(new) {
            return Err(CliError::SetAlreadyExists {
                name: new.to_string(),
            });
        }
        let def = sets.sets.remove(old).ok_or_else(|| CliError::SetNotFound {
            name: old.to_string(),
        })?;
        sets.sets.insert(new.to_string(), def);
        Ok(())
    })?;
    Ok(SetRenameResult {
        old: old.to_string(),
        new: new.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flags::{Force, Purge, RunMode};
    use crate::lockfile;
    use crate::test_support::{TestContext, make_lock_entry_builder, save_lock_with_entry};
    use std::collections::HashSet;

    // --- create ---

    #[test]
    fn create_errors_when_name_exists() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result = create("rust-work", None, None, InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn create_inserts_inactive_empty_set() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("blog", Some("blog work"), None, InstallScope::Global, &ctx).unwrap();

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let def = sets.sets.get("blog").unwrap();
        assert_eq!(def.state, SetState::Inactive);
        assert!(def.members.is_empty());
        assert_eq!(def.description.as_deref(), Some("blog work"));
    }

    // --- create --from-plugin (plugin seeding) ---

    fn marketplace_json(plugins_json: &str) -> String {
        format!(r#"{{"name":"test","plugins":[{plugins_json}]}}"#)
    }

    #[test]
    fn create_from_seeds_members_inactive() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file(
            "/src/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{"name":"my-plugin","agents":["./agents/my-agent.md"],"skills":["./skills/my-skill"]}"#,
            ),
        );
        t.fs.add_file(
            "/src/agents/my-agent.md",
            crate::test_support::agent_content("my-agent", "An agent"),
        );
        t.fs.add_file(
            "/src/skills/my-skill/SKILL.md",
            crate::test_support::skill_content("A skill"),
        );

        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:my-plugin"), InstallScope::Global, &ctx)
                .unwrap();
        assert_eq!(result.member_count, 2);
        assert_eq!(result.seeded_from.as_deref(), Some("guidelines:my-plugin"));

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let def = sets.sets.get("rust-work").unwrap();
        assert_eq!(def.state, SetState::Inactive);
        assert_eq!(def.members.len(), 2);
        assert!(def.members.iter().all(|m| m.source.as_deref() == Some("guidelines")));
        let kinds: HashSet<ArtifactKind> = def.members.iter().map(|m| m.kind).collect();
        assert!(kinds.contains(&ArtifactKind::Agent));
        assert!(kinds.contains(&ArtifactKind::Skill));
    }

    #[test]
    fn create_from_unknown_plugin_errors_and_creates_nothing() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file(
            "/src/.claude-plugin/marketplace.json",
            marketplace_json(r#"{"name":"my-plugin","agents":["./agents/my-agent.md"]}"#),
        );

        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:ghost"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn create_from_remote_plugin_reports_unsupported() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        t.fs.add_file(
            "/src/.claude-plugin/marketplace.json",
            marketplace_json(
                r#"{"name":"remote-plugin","source":{"source":"url","url":"https://example.com"}}"#,
            ),
        );

        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:remote-plugin"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not yet supported"));

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn create_from_unknown_source_errors_and_creates_nothing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("ghost-source:my-plugin"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn create_from_source_without_marketplace_errors() {
        let t = TestContext::new();
        crate::test_support::setup_source(&t.fs, &t.paths, "guidelines", "/src");
        let ctx = t.ctx();
        let result =
            create("rust-work", None, Some("guidelines:my-plugin"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no marketplace"));
    }

    #[test]
    fn create_from_malformed_spec_errors() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = create("rust-work", None, Some("no-colon-here"), InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected <source>:<plugin>"));
    }

    // --- list ---

    #[test]
    fn list_reports_state_and_member_count() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();

        let result = list(InstallScope::Global, &ctx).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "rust-work");
        assert_eq!(result.entries[0].state, SetState::Inactive);
        assert_eq!(result.entries[0].member_count, 0);
        assert_eq!(result.entries[0].footprint_chars, 0);
    }

    // --- footprint ---

    #[test]
    fn list_footprint_reads_installed_members_description() {
        let t = TestContext::new();
        crate::test_support::install_agent_on_disk(
            &t.fs,
            &t.paths,
            "my-agent",
            &crate::test_support::agent_content("my-agent", "Some description"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("my-agent", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        let result = list(InstallScope::Global, &ctx).unwrap();
        assert_eq!(result.entries[0].footprint_chars, "Some description".chars().count());
    }

    #[test]
    fn show_footprint_for_inactive_member_resolves_from_source() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );

        let result = show("rust-work", InstallScope::Global, &ctx).unwrap();
        let expected = "A test agent".chars().count();
        assert_eq!(result.members[0].footprint_chars, Some(expected));
        assert_eq!(result.footprint_chars, expected);
    }

    #[test]
    fn footprint_unresolvable_member_counts_as_zero_and_none() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("ghost", "gone-source")],
            InstallScope::Global,
            &ctx,
        );

        let list_result = list(InstallScope::Global, &ctx).unwrap();
        assert_eq!(list_result.entries[0].footprint_chars, 0);

        let show_result = show("rust-work", InstallScope::Global, &ctx).unwrap();
        assert_eq!(show_result.members[0].footprint_chars, None);
        assert_eq!(show_result.footprint_chars, 0);
    }

    // --- show ---

    #[test]
    fn show_errors_when_missing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = show("nope", InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    // --- add: resolves source from lockfile ---

    #[test]
    fn add_resolves_source_from_lockfile() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "rust-craftsperson",
            make_lock_entry_builder(
                ArtifactKind::Agent,
                "guidelines",
                "agents/rust-craftsperson.md",
            ),
            InstallScope::Global,
        );

        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result =
            add("rust-work", &["rust-craftsperson".to_string()], InstallScope::Global, &ctx)
                .unwrap();
        assert_eq!(result.added, vec!["rust-craftsperson".to_string()]);
        assert!(result.already.is_empty());

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let def = sets.sets.get("rust-work").unwrap();
        assert_eq!(def.members.len(), 1);
        assert_eq!(def.members[0].kind, ArtifactKind::Agent);
        assert_eq!(def.members[0].source.as_deref(), Some("guidelines"));
    }

    #[test]
    fn add_errors_when_artifact_not_installed() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result = add("rust-work", &["ghost".to_string()], InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not installed"));
    }

    #[test]
    fn add_does_not_write_when_any_artifact_unresolvable() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result = add(
            "rust-work",
            &["known".to_string(), "ghost".to_string()],
            InstallScope::Global,
            &ctx,
        );
        assert!(result.is_err());

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(sets.sets.get("rust-work").unwrap().members.is_empty());
    }

    #[test]
    fn add_duplicate_is_noop() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        add("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();
        let result = add("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();
        assert!(result.added.is_empty());
        assert_eq!(result.already, vec!["known".to_string()]);

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert_eq!(sets.sets.get("rust-work").unwrap().members.len(), 1);
    }

    #[test]
    fn add_errors_when_set_missing() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        let result = add("nope", &["known".to_string()], InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn resolve_member_ambiguous_across_kinds_errors() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "dual",
            make_lock_entry_builder(ArtifactKind::Agent, "guidelines", "agents/dual.md"),
            InstallScope::Global,
        );
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "dual",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "dual"),
            InstallScope::Local,
        );

        let ctx = t.ctx();
        create("mixed", None, None, InstallScope::Global, &ctx).unwrap();

        let bare = add("mixed", &["dual".to_string()], InstallScope::Global, &ctx);
        assert!(bare.is_err());
        assert!(bare.unwrap_err().to_string().contains("ambiguous"));

        let prefixed =
            add("mixed", &["skill:dual".to_string()], InstallScope::Global, &ctx).unwrap();
        assert_eq!(prefixed.added, vec!["dual".to_string()]);
    }

    // --- remove ---

    #[test]
    fn remove_drops_member_without_uninstalling() {
        let t = TestContext::new();
        save_lock_with_entry(
            &t.fs,
            &t.paths,
            "known",
            make_lock_entry_builder(ArtifactKind::Skill, "guidelines", "known"),
            InstallScope::Global,
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        add("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();

        let result =
            remove("rust-work", &["known".to_string()], InstallScope::Global, &ctx).unwrap();
        assert_eq!(result.removed, vec!["known".to_string()]);
        assert!(result.not_found.is_empty());

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(sets.sets.get("rust-work").unwrap().members.is_empty());
        // Still tracked in the lockfile — remove does not uninstall.
        let lock = lockfile::load(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(lock.packages.contains_key("known"));
    }

    #[test]
    fn remove_reports_not_found_for_missing_member() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        let result =
            remove("rust-work", &["ghost".to_string()], InstallScope::Global, &ctx).unwrap();
        assert!(result.removed.is_empty());
        assert_eq!(result.not_found, vec!["ghost".to_string()]);
    }

    // --- delete ---

    #[test]
    fn delete_removes_definition() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        delete("rust-work", Purge::No, Force::No, RunMode::Plan, InstallScope::Global, &ctx)
            .unwrap();
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn delete_errors_when_missing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result =
            delete("nope", Purge::No, Force::No, RunMode::Plan, InstallScope::Global, &ctx);
        assert!(result.is_err());
    }

    // --- rename ---

    #[test]
    fn rename_errors_when_old_missing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = rename("nope", "new", InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn rename_errors_when_new_exists() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("old", None, None, InstallScope::Global, &ctx).unwrap();
        create("new", None, None, InstallScope::Global, &ctx).unwrap();
        let result = rename("old", "new", InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn rename_moves_definition_to_new_key() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("old", Some("desc"), None, InstallScope::Global, &ctx).unwrap();
        rename("old", "new", InstallScope::Global, &ctx).unwrap();

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("old"));
        let def = sets.sets.get("new").unwrap();
        assert_eq!(def.description.as_deref(), Some("desc"));
    }

    // --- activate / deactivate ---

    /// Directly seed a set's members (bypassing `add`'s lockfile-resolution
    /// requirement) — Phase 2 tests care about the pinned source, not how the
    /// member first got there.
    fn seed_members(
        set_name: &str,
        members: Vec<SetMember>,
        scope: InstallScope,
        ctx: &AppContext<'_>,
    ) {
        config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| -> Result<()> {
            sets.sets.get_mut(set_name).unwrap().members = members;
            Ok(())
        })
        .unwrap();
    }

    fn pinned_agent(name: &str, source: &str) -> SetMember {
        SetMember {
            kind: ArtifactKind::Agent,
            name: name.to_string(),
            source: Some(source.to_string()),
        }
    }

    #[test]
    fn delete_purge_deactivates_then_deletes() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();

        delete("rust-work", Purge::Yes, Force::No, RunMode::Apply, InstallScope::Global, &ctx)
            .unwrap();

        assert!(!t.paths.is_installed(
            ArtifactKind::Agent,
            "rust-craftsperson",
            InstallScope::Global,
            &t.fs
        ));
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn delete_purge_with_drift_blocked_member_preserves_definition() {
        let t = TestContext::new();
        crate::test_support::setup_source_with_agent(
            &t.fs,
            &t.paths,
            "guidelines",
            "/src",
            "rust-craftsperson",
        );
        let ctx = t.ctx();
        create("rust-work", None, None, InstallScope::Global, &ctx).unwrap();
        seed_members(
            "rust-work",
            vec![pinned_agent("rust-craftsperson", "guidelines")],
            InstallScope::Global,
            &ctx,
        );
        activate("rust-work", RunMode::Apply, InstallScope::Global, &ctx).unwrap();
        let installed_path = t
            .paths
            .installed_artifact_path(ArtifactKind::Agent, "rust-craftsperson", InstallScope::Global)
            .unwrap();
        t.fs.add_file(installed_path.clone(), "edited by hand");

        let result =
            delete("rust-work", Purge::Yes, Force::No, RunMode::Apply, InstallScope::Global, &ctx)
                .unwrap();
        assert!(!result.deleted);
        assert!(t.fs.file_exists(&installed_path));
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(
            sets.sets.contains_key("rust-work"),
            "definition preserved when purge is blocked"
        );

        // --force forwards through and completes the purge.
        delete("rust-work", Purge::Yes, Force::Yes, RunMode::Apply, InstallScope::Global, &ctx)
            .unwrap();
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }
}
