//! `cmx set` — Phase 1: definitions, curation, and inspection of sets.
//!
//! A set is a locally-defined, named group of installed artifacts with a
//! desired activation state (see `SETS.md`). Phase 1 covers `create`, `list`,
//! `show`, `add`, `remove`, `delete`, and `rename` — pure curation, no
//! install/uninstall side effects. `activate`/`deactivate` land in Phase 2;
//! context-footprint reporting and `doctor` integration in Phase 3.

use anyhow::{Result, bail};
use std::collections::HashSet;

use crate::config;
use crate::context::AppContext;
use crate::lockfile;
use crate::types::{ArtifactKind, InstallScope, SetDef, SetMember, SetState};

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct SetCreateResult {
    pub name: String,
}

#[derive(Debug)]
pub struct SetListEntry {
    pub name: String,
    pub state: SetState,
    pub member_count: usize,
}

#[derive(Debug)]
pub struct SetListResult {
    pub entries: Vec<SetListEntry>,
}

#[derive(Debug)]
pub struct SetMemberStatus {
    pub kind: ArtifactKind,
    pub name: String,
    pub source: Option<String>,
    pub installed: bool,
}

#[derive(Debug)]
pub struct SetShowResult {
    pub name: String,
    pub description: Option<String>,
    pub state: SetState,
    pub members: Vec<SetMemberStatus>,
}

#[derive(Debug)]
pub struct SetAddResult {
    pub set: String,
    pub added: Vec<String>,
    pub already: Vec<String>,
}

#[derive(Debug)]
pub struct SetRemoveResult {
    pub set: String,
    pub removed: Vec<String>,
    pub not_found: Vec<String>,
}

#[derive(Debug)]
pub struct SetDeleteResult {
    pub name: String,
}

#[derive(Debug)]
pub struct SetRenameResult {
    pub old: String,
    pub new: String,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn create(
    name: &str,
    description: Option<&str>,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetCreateResult> {
    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        if sets.sets.contains_key(name) {
            bail!("Set '{name}' already exists.");
        }
        sets.sets.insert(
            name.to_string(),
            SetDef {
                description: description.map(str::to_string),
                state: SetState::Inactive,
                members: Vec::new(),
            },
        );
        Ok(())
    })?;
    Ok(SetCreateResult {
        name: name.to_string(),
    })
}

pub fn list(scope: InstallScope, ctx: &AppContext<'_>) -> Result<SetListResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let entries = sets
        .sets
        .into_iter()
        .map(|(name, def)| SetListEntry {
            name,
            state: def.state,
            member_count: def.members.len(),
        })
        .collect();
    Ok(SetListResult { entries })
}

pub fn show(name: &str, scope: InstallScope, ctx: &AppContext<'_>) -> Result<SetShowResult> {
    let sets = config::load_sets(scope, ctx.fs, ctx.paths)?;
    let def = sets.sets.get(name).ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;

    let members = def
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
            }
        })
        .collect();

    Ok(SetShowResult {
        name: name.to_string(),
        description: def.description.clone(),
        state: def.state,
        members,
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

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        let def = sets
            .sets
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;

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

    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        let def = sets
            .sets
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;

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

pub fn delete(name: &str, scope: InstallScope, ctx: &AppContext<'_>) -> Result<SetDeleteResult> {
    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        sets.sets
            .remove(name)
            .ok_or_else(|| anyhow::anyhow!("Set '{name}' not found."))?;
        Ok(())
    })?;
    Ok(SetDeleteResult {
        name: name.to_string(),
    })
}

pub fn rename(
    old: &str,
    new: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<SetRenameResult> {
    config::mutate_sets(scope, ctx.fs, ctx.paths, |sets| {
        if sets.sets.contains_key(new) {
            bail!("Set '{new}' already exists.");
        }
        let def = sets.sets.remove(old).ok_or_else(|| anyhow::anyhow!("Set '{old}' not found."))?;
        sets.sets.insert(new.to_string(), def);
        Ok(())
    })?;
    Ok(SetRenameResult {
        old: old.to_string(),
        new: new.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Split an optional `skill:`/`agent:` disambiguation prefix off an artifact
/// argument. `source:name` disambiguation is deferred (SETS.md Phase 4).
fn parse_prefix(arg: &str) -> (Option<ArtifactKind>, &str) {
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
fn resolve_member(arg: &str, ctx: &AppContext<'_>) -> Result<SetMember> {
    let (hint, name) = parse_prefix(arg);

    let mut candidates: Vec<(ArtifactKind, String)> = Vec::new();
    for scope in InstallScope::ALL {
        let lock = lockfile::load(scope, ctx.fs, ctx.paths)?;
        if let Some(entry) = lock.packages.get(name) {
            candidates.push((entry.artifact_type, entry.source.repo.clone()));
        }
    }

    if candidates.is_empty() {
        bail!(
            "Artifact '{name}' is not installed (no lockfile entry); cannot resolve its kind/source."
        );
    }

    let candidates: Vec<(ArtifactKind, String)> = match hint {
        Some(k) => candidates.into_iter().filter(|(kind, _)| *kind == k).collect(),
        None => candidates,
    };

    if candidates.is_empty() {
        bail!("Artifact '{name}' has no lockfile entry matching the requested kind.");
    }

    let distinct_kinds: HashSet<ArtifactKind> = candidates.iter().map(|(k, _)| *k).collect();
    if hint.is_none() && distinct_kinds.len() > 1 {
        bail!(
            "'{name}' is ambiguous across kinds — use skill:{name} or agent:{name} to disambiguate."
        );
    }

    let (kind, source) = candidates.into_iter().next().expect("non-empty, checked above");
    Ok(SetMember {
        kind,
        name: name.to_string(),
        source: Some(source),
    })
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TestContext, make_lock_entry_builder, save_lock_with_entry};

    // --- create ---

    #[test]
    fn create_errors_when_name_exists() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
        let result = create("rust-work", None, InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn create_inserts_inactive_empty_set() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("blog", Some("blog work"), InstallScope::Global, &ctx).unwrap();

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        let def = sets.sets.get("blog").unwrap();
        assert_eq!(def.state, SetState::Inactive);
        assert!(def.members.is_empty());
        assert_eq!(def.description.as_deref(), Some("blog work"));
    }

    // --- list ---

    #[test]
    fn list_reports_state_and_member_count() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();

        let result = list(InstallScope::Global, &ctx).unwrap();
        assert_eq!(result.entries.len(), 1);
        assert_eq!(result.entries[0].name, "rust-work");
        assert_eq!(result.entries[0].state, SetState::Inactive);
        assert_eq!(result.entries[0].member_count, 0);
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
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
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
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
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
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
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
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
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
        create("mixed", None, InstallScope::Global, &ctx).unwrap();

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
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
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
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
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
        create("rust-work", None, InstallScope::Global, &ctx).unwrap();
        delete("rust-work", InstallScope::Global, &ctx).unwrap();
        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("rust-work"));
    }

    #[test]
    fn delete_errors_when_missing() {
        let t = TestContext::new();
        let ctx = t.ctx();
        let result = delete("nope", InstallScope::Global, &ctx);
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
        create("old", None, InstallScope::Global, &ctx).unwrap();
        create("new", None, InstallScope::Global, &ctx).unwrap();
        let result = rename("old", "new", InstallScope::Global, &ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn rename_moves_definition_to_new_key() {
        let t = TestContext::new();
        let ctx = t.ctx();
        create("old", Some("desc"), InstallScope::Global, &ctx).unwrap();
        rename("old", "new", InstallScope::Global, &ctx).unwrap();

        let sets = config::load_sets(InstallScope::Global, &t.fs, &t.paths).unwrap();
        assert!(!sets.sets.contains_key("old"));
        let def = sets.sets.get("new").unwrap();
        assert_eq!(def.description.as_deref(), Some("desc"));
    }
}
