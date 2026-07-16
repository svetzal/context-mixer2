use std::path::PathBuf;

use crate::skill_fs::SkillFile;
use crate::test_support::TestContext;
use crate::types::{ArtifactKind, InstallScope, LockEntry, LockSource};

use super::{BundledSkill, InstallPlan, Scope, SkillInstaller, ToolIdentity};
use crate::platform::Platform;

pub fn make_file(rel: &str, content: &str) -> SkillFile {
    SkillFile {
        rel_path: PathBuf::from(rel),
        bytes: content.as_bytes().to_vec(),
    }
}

// Uses the canonical `metadata.version` frontmatter form so that cmx-core's
// auto-stamp (see `frontmatter::reconcile_skill_version`) is idempotent on it:
// the bundled bytes already equal what the installer would write, keeping the
// checksum fixtures below stable.
pub fn sample_skill(version: &str) -> BundledSkill {
    BundledSkill::from_files(vec![
        make_file(
            "SKILL.md",
            &format!("---\nmetadata:\n  version: \"{version}\"\n---\n# Sample skill\n"),
        ),
        make_file("scripts/tool.py", "print('hello')"),
    ])
}

pub fn installer(version: &str) -> SkillInstaller {
    SkillInstaller::new(ToolIdentity {
        name: "sample".to_string(),
        version: version.to_string(),
    })
}

pub fn plan_with_locked_version(
    t: &TestContext,
    locked_version: &str,
    locked_checksum: &str,
    bundled_version: &str,
    force: bool,
) -> InstallPlan {
    // Set up a lock entry for Claude with the given version and checksum.
    let claude_paths = t.paths.with_platform(Platform::Claude);
    let skill_dir = claude_paths
        .install_dir(ArtifactKind::Skill, InstallScope::Global)
        .unwrap()
        .join("sample");
    t.fs.add_dir(skill_dir.clone());

    crate::test_support::save_lock_with_entry(
        &t.fs,
        &claude_paths,
        "sample",
        LockEntry {
            artifact_type: ArtifactKind::Skill,
            version: Some(locked_version.to_string()),
            installed_at: "2024-01-01T00:00:00Z".to_string(),
            source: LockSource {
                repo: "bundled:sample".to_string(),
                path: "skills/sample".to_string(),
            },
            source_checksum: locked_checksum.to_string(),
            installed_checksum: locked_checksum.to_string(),
        },
        InstallScope::Global,
    );
    let skill = sample_skill(bundled_version);
    let ctx = t.ctx();
    installer(bundled_version).plan(&skill, Scope::Global, force, &ctx).unwrap()
}
