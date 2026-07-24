//! Lock-state reconciliation: `focus_lock_state`, `reconciliations`.

use crate::error::Result;

use crate::context::AppContext;
use crate::lockfile;
use crate::platform::Platform;
use crate::types::InstallScope;

use super::discovery::InstalledCopy;
use super::{FocusedComparison, Reconciliation};

/// Read the focus copy's lock baseline (from any platform that reads it): its
/// recorded version, and whether the copy was edited after install (its bytes no
/// longer match the lock's checksum).
pub(super) fn focus_lock_state(
    name: &str,
    copy: &InstalledCopy,
    checksum: &str,
    scope: InstallScope,
    ctx: &AppContext<'_>,
) -> Result<(Option<String>, bool)> {
    for &platform in &copy.platforms {
        let pv = ctx.paths.with_platform(platform);
        if let Some(entry) = lockfile::load(scope, ctx.fs, &pv)?.packages.get(name) {
            return Ok((entry.version.clone(), entry.installed_checksum != checksum));
        }
    }
    Ok((None, false))
}

/// Build the reconciliation directions, naming the two copies concretely
/// (`{changed}` is the edited platform copy, `{source_name}` the canonical one).
/// When the source is the home, `{changed}`'s edits can be promoted into it;
/// either way they can be discarded by re-installing. `diff` never picks for the
/// user. `platform`, set when copies span platforms, qualifies the commands.
pub(super) fn reconciliations(
    cmp: &FocusedComparison<'_>,
    locally_modified: bool,
    platform: Option<Platform>,
) -> Vec<Reconciliation> {
    let mut out = Vec::new();
    let source_is_home = cmp.source_name == crate::adopt::HOME_SOURCE;
    let promote_plat = platform.map(|p| format!(" --from {p}")).unwrap_or_default();
    let update_plat = platform.map(|p| format!(" --platform {p}")).unwrap_or_default();
    let name = cmp.name;
    let kind = cmp.kind;
    let source_name = cmp.source_name;
    let changed = cmp.changed_label;

    if source_is_home {
        out.push(Reconciliation {
            description: format!("keep {changed}'s edits — copy {changed} into {source_name}"),
            command: format!("cmx {kind} promote {name}{promote_plat}"),
            note: None,
        });
    }

    out.push(Reconciliation {
        description: format!("discard {changed}'s edits — restore {changed} from {source_name}"),
        command: if locally_modified {
            format!("cmx {kind} update {name}{update_plat} --force")
        } else {
            format!("cmx {kind} update {name}{update_plat}")
        },
        note: locally_modified.then(|| format!("--force overwrites {changed}'s local edits")),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::Platform;
    use crate::types::ArtifactKind;

    // --- reconciliations ---

    #[test]
    fn reconciliations_offers_promote_when_source_is_home() {
        let cmp = FocusedComparison {
            name: "pf",
            kind: ArtifactKind::Skill,
            source_name: "home",
            changed_label: "claude",
            source_version: None,
            changed_version: None,
        };
        let rs = reconciliations(&cmp, true, None);
        assert_eq!(rs.len(), 2, "promote + update");
        assert!(rs[0].command.contains("promote pf"), "promote offered first: {:?}", rs[0]);
        assert!(rs[1].command.contains("update pf --force"), "update with force: {:?}", rs[1]);
        // Descriptions name the concrete copies, not "installed"/"source".
        assert!(
            rs[0].description.contains("claude") && rs[0].description.contains("home"),
            "{:?}",
            rs[0]
        );
        assert!(!rs[0].description.contains("installed"), "no abstract 'installed': {:?}", rs[0]);
        assert!(
            rs[1].note.as_deref().unwrap().contains("claude"),
            "caveat names the copy: {:?}",
            rs[1]
        );
    }

    #[test]
    fn reconciliations_no_promote_for_git_source() {
        let cmp = FocusedComparison {
            name: "slidev",
            kind: ArtifactKind::Skill,
            source_name: "guidelines",
            changed_label: "codex",
            source_version: None,
            changed_version: None,
        };
        let rs = reconciliations(&cmp, false, None);
        assert_eq!(rs.len(), 1, "only restore-from-source for a git source");
        assert!(rs[0].command.contains("update slidev"), "{:?}", rs[0]);
        assert!(!rs[0].command.contains("--force"), "no force when not locally modified");
        assert!(rs[0].description.contains("guidelines"), "names the source: {:?}", rs[0]);
        assert!(rs[0].description.contains("codex"), "names the changed copy: {:?}", rs[0]);
    }

    #[test]
    fn reconciliations_qualify_commands_with_platform_when_multi() {
        let cmp = FocusedComparison {
            name: "pf",
            kind: ArtifactKind::Skill,
            source_name: "home",
            changed_label: "codex",
            source_version: None,
            changed_version: None,
        };
        let rs = reconciliations(&cmp, true, Some(Platform::Codex));
        assert!(rs[0].command.contains("promote pf --from codex"), "{:?}", rs[0]);
        assert!(rs[1].command.contains("update pf --platform codex --force"), "{:?}", rs[1]);
    }
}
