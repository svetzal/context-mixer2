use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config;
use crate::frontmatter;
use crate::lockfile;
use crate::platform::Platform;
use crate::skill_fs::{self, SkillFile};
use crate::skill_install::{
    BundledSkill, InstallPlan, Report, Scope, SkillInstaller, TargetAction, ToolIdentity,
};
use crate::targets;
use crate::test_support::TestContext;
use crate::types::{ArtifactKind, CmxConfig, InstallScope, LockEntry, LockFile, LockSource};

const FIXTURE_TOOL_NAME: &str = "fixture-tool";
const FIXTURE_VERSION: &str = "2.4.6";

/// Generate the committed language-neutral conformance fixtures under `out`.
///
/// The observable behavior always comes from the in-memory `test-support`
/// oracle and a fixed injected clock. `out` is the only real filesystem write.
pub fn generate_conformance_fixtures(out: &Path) -> Result<()> {
    recreate_dir(out)?;
    write_readme(&out.join("README.md"))?;
    generate_checksum_fixtures(&out.join("checksum"))?;
    generate_frontmatter_fixtures(&out.join("frontmatter"))?;
    generate_version_guard_fixtures(&out.join("version-guard"))?;
    generate_paths_fixtures(&out.join("paths"))?;
    generate_target_resolve_fixtures(&out.join("target-resolve"))?;
    generate_install_e2e_fixtures(&out.join("install-e2e"))?;
    Ok(())
}

fn fixed_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 5, 12, 0, 0)
        .single()
        .expect("fixed conformance timestamp is valid")
}

fn fixed_timestamp() -> String {
    fixed_time().to_rfc3339()
}

fn recreate_dir(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("remove existing fixture dir {}", path.display()))?;
    }
    fs::create_dir_all(path).with_context(|| format!("create fixture dir {}", path.display()))?;
    Ok(())
}

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create parent dir {}", parent.display()))?;
    }
    Ok(())
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    ensure_parent(path)?;
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn write_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let bytes = serde_json::to_vec_pretty(value)?;
    write_bytes(path, &bytes)
}

fn normalized_path_string(path: &Path) -> String {
    use std::path::Component;
    // Join the *non-root* components with `/`, re-adding a single leading slash
    // for absolute paths. Joining every component (including `Component::RootDir`,
    // which stringifies to `/`) would emit a doubled leading slash — `//home/...`
    // — for absolute dest_dirs.
    let mut absolute = false;
    let parts: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::RootDir => {
                absolute = true;
                None
            }
            other => Some(other.as_os_str().to_string_lossy().into_owned()),
        })
        .collect();
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

fn bundle() -> BundledSkill {
    BundledSkill::from_files(vec![
        SkillFile::text(
            "SKILL.md",
            "---\nname: fixture-tool\ndescription: Fixture skill\nmetadata:\n  author: Fixture Bot\n---\n# Fixture skill\n",
        ),
        SkillFile::text("scripts/tool.py", "print('fixture tool')\n"),
    ])
}

fn tool_identity(version: &str) -> ToolIdentity {
    ToolIdentity::new(FIXTURE_TOOL_NAME, version)
}

fn installer(version: &str) -> SkillInstaller {
    SkillInstaller::new(tool_identity(version))
}

fn reconciled_files(skill: &BundledSkill, version: &str) -> Vec<SkillFile> {
    frontmatter::reconcile_skill_version(&skill.files, version)
}

fn bundled_checksum(skill: &BundledSkill, version: &str) -> String {
    skill_fs::checksum_bundled(&reconciled_files(skill, version))
}

fn skill_dest(paths: &crate::paths::ConfigPaths, scope: InstallScope) -> PathBuf {
    paths
        .with_platform(Platform::Claude)
        .require_install_dir(ArtifactKind::Skill, scope)
        .expect("claude supports skill installs")
        .join(FIXTURE_TOOL_NAME)
}

fn bundled_lock_entry(version: Option<&str>, checksum: &str) -> LockEntry {
    LockEntry {
        artifact_type: ArtifactKind::Skill,
        version: version.map(str::to_string),
        installed_at: fixed_timestamp(),
        source: LockSource {
            repo: format!("bundled:{FIXTURE_TOOL_NAME}"),
            path: format!("skills/{FIXTURE_TOOL_NAME}"),
        },
        source_checksum: checksum.to_string(),
        installed_checksum: checksum.to_string(),
    }
}

fn save_bundled_lock(
    test: &TestContext,
    scope: InstallScope,
    version: Option<&str>,
    checksum: &str,
) -> Result<()> {
    let mut packages = BTreeMap::new();
    packages.insert(FIXTURE_TOOL_NAME.to_string(), bundled_lock_entry(version, checksum));
    let lock = LockFile {
        version: 1,
        packages,
    };
    let paths = test.paths.with_platform(Platform::Claude);
    lockfile::save(&lock, scope, &test.fs, &paths)
}

fn write_bundle_version(
    test: &TestContext,
    scope: InstallScope,
    skill: &BundledSkill,
    version: &str,
) -> Result<String> {
    let files = reconciled_files(skill, version);
    let checksum = skill_fs::checksum_bundled(&files);
    skill_fs::write_skill_files(&skill_dest(&test.paths, scope), &files, &test.fs)?;
    Ok(checksum)
}

fn write_drifted_bundle_version(
    test: &TestContext,
    scope: InstallScope,
    skill: &BundledSkill,
    version: &str,
) -> Result<String> {
    let mut files = reconciled_files(skill, version);
    let checksum = skill_fs::checksum_bundled(&files);
    let drifted = files
        .iter_mut()
        .find(|file| file.rel_path == Path::new("scripts/tool.py"))
        .context("bundle contains scripts/tool.py")?;
    drifted.bytes = b"print('drifted local edit')\n".to_vec();
    skill_fs::write_skill_files(&skill_dest(&test.paths, scope), &files, &test.fs)?;
    Ok(checksum)
}

fn all_lock_paths(paths: &crate::paths::ConfigPaths, scope: InstallScope) -> BTreeSet<PathBuf> {
    Platform::ALL
        .iter()
        .map(|platform| paths.with_platform(*platform).lock_path(scope))
        .collect()
}

fn normalized_fixture_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.strip_prefix(Path::new("/"))
            .expect("absolute path always strips root")
            .to_path_buf()
    } else {
        PathBuf::from("project").join(path)
    }
}

fn snapshot_tree(test: &TestContext, scope: InstallScope) -> BTreeMap<PathBuf, Vec<u8>> {
    let lock_paths = all_lock_paths(&test.paths, scope);
    test.fs
        .snapshot_files()
        .into_iter()
        .filter(|(path, _)| !lock_paths.contains(path))
        .map(|(path, bytes)| (normalized_fixture_path(&path), bytes))
        .collect()
}

fn snapshot_locks(test: &TestContext, scope: InstallScope) -> Result<BTreeMap<String, Value>> {
    let mut locks = BTreeMap::new();
    for platform in Platform::ALL {
        let path = test.paths.with_platform(platform).lock_path(scope);
        if let Some(bytes) = test.fs.get_file_content(&path) {
            let name = path
                .file_name()
                .context("lock path has file name")?
                .to_string_lossy()
                .to_string();
            let value = serde_json::from_slice::<Value>(&bytes)
                .with_context(|| format!("parse fake lock {}", path.display()))?;
            locks.insert(name, value);
        }
    }
    Ok(locks)
}

fn write_tree_snapshot(root: &Path, files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<()> {
    fs::create_dir_all(root).with_context(|| format!("create {}", root.display()))?;
    for (rel_path, bytes) in files {
        write_bytes(&root.join(rel_path), bytes)?;
    }
    Ok(())
}

fn write_lock_snapshot(root: &Path, locks: &BTreeMap<String, Value>) -> Result<()> {
    fs::create_dir_all(root).with_context(|| format!("create {}", root.display()))?;
    for (name, value) in locks {
        write_json(&root.join(name), value)?;
    }
    Ok(())
}

fn action_kind(action: &TargetAction) -> &'static str {
    match action {
        TargetAction::Install => "install",
        TargetAction::Update { .. } => "update",
        TargetAction::Skip => "skip",
        TargetAction::DriftedSkip { .. } => "drifted-skip",
        TargetAction::RefuseNewer { .. } => "refuse-newer",
        TargetAction::Downgrade { .. } => "downgrade",
    }
}

#[derive(Serialize)]
struct ActionSnapshot {
    kind: String,
    from: Option<String>,
    installed: Option<String>,
    will_write: bool,
    blocked: bool,
}

fn snapshot_action(action: &TargetAction) -> ActionSnapshot {
    match action {
        TargetAction::Install | TargetAction::Skip => ActionSnapshot {
            kind: action_kind(action).to_string(),
            from: None,
            installed: None,
            will_write: action.will_write(),
            blocked: action.is_blocked(),
        },
        TargetAction::Update { from } => ActionSnapshot {
            kind: action_kind(action).to_string(),
            from: from.clone(),
            installed: None,
            will_write: action.will_write(),
            blocked: action.is_blocked(),
        },
        TargetAction::DriftedSkip { installed } | TargetAction::RefuseNewer { installed } => {
            ActionSnapshot {
                kind: action_kind(action).to_string(),
                from: None,
                installed: Some(installed.clone()),
                will_write: action.will_write(),
                blocked: action.is_blocked(),
            }
        }
        TargetAction::Downgrade { from } => ActionSnapshot {
            kind: action_kind(action).to_string(),
            from: Some(from.clone()),
            installed: None,
            will_write: action.will_write(),
            blocked: action.is_blocked(),
        },
    }
}

#[derive(Serialize)]
struct ChecksumManifest {
    schema_version: u32,
    cases: Vec<ChecksumCase>,
}

#[derive(Serialize)]
struct ChecksumCase {
    name: String,
    description: String,
    input: InlineFilesInput,
    expected: ChecksumExpected,
}

#[derive(Serialize)]
struct InlineFilesInput {
    files: Vec<TextFixtureFile>,
}

#[derive(Serialize)]
struct TextFixtureFile {
    path: String,
    content_utf8: String,
}

#[derive(Serialize)]
struct ChecksumExpected {
    sha256: String,
    canonical_order: Vec<String>,
    canonical_included_paths: Vec<String>,
}

fn checksum_case(name: &str, description: &str, files: Vec<(&str, &str)>) -> ChecksumCase {
    let skill_files = files
        .iter()
        .map(|(path, content)| SkillFile::text(path, content))
        .collect::<Vec<_>>();
    let canonical = skill_fs::canonical_files(&skill_files);
    let canonical_order = canonical
        .iter()
        .map(|file| normalized_path_string(&file.rel_path))
        .collect::<Vec<_>>();
    let checksum = skill_fs::checksum_bundled(&skill_files);

    ChecksumCase {
        name: name.to_string(),
        description: description.to_string(),
        input: InlineFilesInput {
            files: files
                .into_iter()
                .map(|(path, content)| TextFixtureFile {
                    path: path.to_string(),
                    content_utf8: content.to_string(),
                })
                .collect(),
        },
        expected: ChecksumExpected {
            sha256: checksum,
            canonical_order: canonical_order.clone(),
            canonical_included_paths: canonical_order,
        },
    }
}

fn generate_checksum_fixtures(out: &Path) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;

    let string_sort = checksum_case(
        "string-sort-a-dot-slash",
        "Pins SPEC §11.4 string sorting for `a`, `a.b`, and `a/b` using an inline file set that cannot exist on disk.",
        vec![("a/b", "nested\n"), ("a.b", "dotted\n"), ("a", "bare\n")],
    );
    ensure!(
        string_sort.expected.canonical_order == vec!["a", "a.b", "a/b"],
        "checksum oracle no longer sorts `a`, `a.b`, `a/b` as SPEC §11.4 requires"
    );

    let filter = checksum_case(
        "canonical-filter",
        "Exercises dotfile, dotdir, transient-name, and `.pyc` filtering while leaving canonical files intact.",
        vec![
            ("SKILL.md", "# skill\n"),
            ("scripts/tool.py", "print('ok')\n"),
            (".hidden", "secret\n"),
            ("scripts/.env", "TOKEN=1\n"),
            ("node_modules/pkg/index.js", "vendored\n"),
            ("scripts/node_modules/pkg/index.js", "nested vendored\n"),
            ("__pycache__/tool.cpython-313.pyc", "compiled\n"),
            ("scripts/tool.pyc", "compiled\n"),
            (".DS_Store", "mac metadata\n"),
            ("nested/.git/config", "[core]\n"),
        ],
    );

    let manifest = ChecksumManifest {
        schema_version: 1,
        cases: vec![string_sort, filter],
    };
    write_json(&out.join("manifest.json"), &manifest)
}

#[derive(Serialize)]
struct FrontmatterManifest {
    schema_version: u32,
    cases: Vec<FrontmatterCase>,
}

#[derive(Serialize)]
struct FrontmatterCase {
    name: String,
    description: String,
    input: FrontmatterInput,
    expected: FrontmatterExpected,
}

#[derive(Serialize)]
struct FrontmatterInput {
    version: String,
    skill_md_path: String,
}

#[derive(Serialize)]
struct FrontmatterExpected {
    skill_md_path: String,
    byte_exact: bool,
    idempotent_second_pass: bool,
}

struct FrontmatterFixture<'a> {
    name: &'a str,
    description: &'a str,
    version: &'a str,
    input: Vec<u8>,
    idempotent_second_pass: bool,
}

fn reconcile_skill_md_bytes(input: &[u8], version: &str) -> Result<Vec<u8>> {
    let files = vec![SkillFile {
        rel_path: PathBuf::from("SKILL.md"),
        bytes: input.to_vec(),
    }];
    let reconciled = frontmatter::reconcile_skill_version(&files, version);
    reconciled
        .into_iter()
        .find(|file| file.rel_path == Path::new("SKILL.md"))
        .map(|file| file.bytes)
        .context("reconciled bundle still contains SKILL.md")
}

fn generate_frontmatter_fixtures(out: &Path) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;

    let fixtures = vec![
        FrontmatterFixture {
            name: "no-fence",
            description: "Leaves SKILL.md unchanged when no leading frontmatter fence exists.",
            version: FIXTURE_VERSION,
            input: b"# No frontmatter\n\nBody only.\n".to_vec(),
            idempotent_second_pass: false,
        },
        FrontmatterFixture {
            name: "empty-metadata-block",
            description: "Adds `metadata.version` using the default two-space indent when the metadata block is empty.",
            version: FIXTURE_VERSION,
            input: b"---\nname: Empty metadata\nmetadata:\n---\n# Body\n".to_vec(),
            idempotent_second_pass: false,
        },
        FrontmatterFixture {
            name: "existing-metadata-version",
            description: "Replaces an existing `metadata.version` value in place.",
            version: FIXTURE_VERSION,
            input: b"---\nname: Existing\nmetadata:\n  version: \"0.0.0\"\n  author: Test\n---\n# Body\n".to_vec(),
            idempotent_second_pass: false,
        },
        FrontmatterFixture {
            name: "shadowing-top-level-version",
            description: "Removes a shadowing top-level `version:` key and writes `metadata.version` instead.",
            version: FIXTURE_VERSION,
            input: b"---\nname: Shadow\nversion: 9.9.9\nmetadata:\n  author: Test\n---\n# Body\n".to_vec(),
            idempotent_second_pass: false,
        },
        FrontmatterFixture {
            name: "crlf-line-endings",
            description: "Captures the reference output for CRLF frontmatter input exactly as emitted by the Rust oracle.",
            version: FIXTURE_VERSION,
            input: b"---\r\nname: CrLf\r\nmetadata:\r\n  version: \"0.0.0\"\r\n---\r\n# Body\r\n".to_vec(),
            idempotent_second_pass: false,
        },
        FrontmatterFixture {
            name: "metadata-indent-four-spaces",
            description: "Inserts `metadata.version` using the indentation of the metadata block's first child.",
            version: FIXTURE_VERSION,
            input: b"---\nmetadata:\n    author: Test\n---\n# Body\n".to_vec(),
            idempotent_second_pass: false,
        },
        FrontmatterFixture {
            name: "folded-description-before-version",
            description: "Updates `metadata.version` without disturbing a folded `description:` block above it.",
            version: FIXTURE_VERSION,
            input: b"---\nmetadata:\n  description: >\n    line one\n    line two\n  version: \"0.0.0\"\n---\n# Body\n".to_vec(),
            idempotent_second_pass: false,
        },
        FrontmatterFixture {
            name: "idempotent-already-reconciled",
            description: "Re-running reconciliation against already-correct bytes yields the same byte stream.",
            version: FIXTURE_VERSION,
            input: format!(
                "---\nmetadata:\n  version: \"{FIXTURE_VERSION}\"\n  author: Test\n---\n# Body\n"
            )
            .into_bytes(),
            idempotent_second_pass: true,
        },
    ];

    let mut manifest_cases = Vec::new();
    for fixture in fixtures {
        let case_dir = out.join(fixture.name);
        let input_path = case_dir.join("input").join("SKILL.md");
        let expected_path = case_dir.join("expected").join("SKILL.md");
        let expected_bytes = reconcile_skill_md_bytes(&fixture.input, fixture.version)?;

        write_bytes(&input_path, &fixture.input)?;
        write_bytes(&expected_path, &expected_bytes)?;

        if fixture.idempotent_second_pass {
            let second_pass = reconcile_skill_md_bytes(&expected_bytes, fixture.version)?;
            ensure!(
                second_pass == expected_bytes,
                "frontmatter idempotency fixture `{}` no longer round-trips byte-for-byte",
                fixture.name
            );
        }

        manifest_cases.push(FrontmatterCase {
            name: fixture.name.to_string(),
            description: fixture.description.to_string(),
            input: FrontmatterInput {
                version: fixture.version.to_string(),
                skill_md_path: format!("{}/input/SKILL.md", fixture.name),
            },
            expected: FrontmatterExpected {
                skill_md_path: format!("{}/expected/SKILL.md", fixture.name),
                byte_exact: true,
                idempotent_second_pass: fixture.idempotent_second_pass,
            },
        });
    }

    let manifest = FrontmatterManifest {
        schema_version: 1,
        cases: manifest_cases,
    };
    write_json(&out.join("manifest.json"), &manifest)
}

#[derive(Serialize)]
struct VersionGuardManifest {
    schema_version: u32,
    cases: Vec<VersionGuardCase>,
}

#[derive(Serialize)]
struct VersionGuardCase {
    name: String,
    description: String,
    input: VersionGuardInput,
    expected: ActionSnapshot,
}

#[derive(Serialize)]
struct VersionGuardInput {
    bundled_version: String,
    tracked: bool,
    installed_version: Option<String>,
    disk_state: String,
    force: bool,
}

fn observe_version_guard(
    bundled_version: &str,
    tracked: bool,
    installed_version: Option<&str>,
    disk_state: &str,
    force: bool,
) -> Result<ActionSnapshot> {
    let test = TestContext::at(fixed_time());
    let skill = bundle();
    let checksum = bundled_checksum(&skill, bundled_version);

    if tracked {
        save_bundled_lock(&test, InstallScope::Global, installed_version, &checksum)?;
    }

    match disk_state {
        "missing" => {}
        "matches-source" => {
            write_bundle_version(&test, InstallScope::Global, &skill, bundled_version)?;
        }
        "drifted" => {
            write_drifted_bundle_version(&test, InstallScope::Global, &skill, bundled_version)?;
        }
        other => bail!("unknown version-guard disk state `{other}`"),
    }

    let plan = installer(bundled_version).plan(&skill, Scope::Global, force, &test.ctx())?;
    ensure!(
        plan.targets.len() == 1,
        "version-guard fixtures expect a single target, got {}",
        plan.targets.len()
    );
    Ok(snapshot_action(&plan.targets[0].action))
}

fn version_guard_semver_install_inputs() -> Vec<(&'static str, &'static str, VersionGuardInput)> {
    vec![
        (
            "untracked-install",
            "No lock entry always plans an install.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: false,
                installed_version: None,
                disk_state: "missing".to_string(),
                force: false,
            },
        ),
        (
            "tracked-version-absent-update",
            "A tracked entry with no installed version compares as Less and updates.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: None,
                disk_state: "matches-source".to_string(),
                force: false,
            },
        ),
        (
            "older-update",
            "An older installed semver version updates.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: Some("1.9.0".to_string()),
                disk_state: "matches-source".to_string(),
                force: false,
            },
        ),
        (
            "equal-missing-disk-install",
            "An equal tracked version with no on-disk copy installs again.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: Some(FIXTURE_VERSION.to_string()),
                disk_state: "missing".to_string(),
                force: false,
            },
        ),
        (
            "equal-same-skip",
            "An equal tracked version with matching bytes skips.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: Some(FIXTURE_VERSION.to_string()),
                disk_state: "matches-source".to_string(),
                force: false,
            },
        ),
        (
            "equal-drifted-skip",
            "An equal tracked version with local edits drift-skips when force is false.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: Some(FIXTURE_VERSION.to_string()),
                disk_state: "drifted".to_string(),
                force: false,
            },
        ),
        (
            "equal-drifted-force-update",
            "An equal tracked version with local edits updates when force is true.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: Some(FIXTURE_VERSION.to_string()),
                disk_state: "drifted".to_string(),
                force: true,
            },
        ),
    ]
}

fn version_guard_semver_newer_inputs() -> Vec<(&'static str, &'static str, VersionGuardInput)> {
    vec![
        (
            "newer-refuse",
            "A newer installed semver version blocks the plan without force.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: Some("9.0.0".to_string()),
                disk_state: "matches-source".to_string(),
                force: false,
            },
        ),
        (
            "newer-force-downgrade",
            "A newer installed semver version downgrades when force is true.",
            VersionGuardInput {
                bundled_version: FIXTURE_VERSION.to_string(),
                tracked: true,
                installed_version: Some("9.0.0".to_string()),
                disk_state: "matches-source".to_string(),
                force: true,
            },
        ),
    ]
}

fn version_guard_non_semver_inputs() -> Vec<(&'static str, &'static str, VersionGuardInput)> {
    vec![
        (
            "non-semver-equal-fallback",
            "Equal non-semver strings fall back to string equality and then to normal equal-version handling.",
            VersionGuardInput {
                bundled_version: "dev-build".to_string(),
                tracked: true,
                installed_version: Some("dev-build".to_string()),
                disk_state: "matches-source".to_string(),
                force: false,
            },
        ),
        (
            "non-semver-unequal-fallback",
            "Unequal non-semver strings fall back to Less and update.",
            VersionGuardInput {
                bundled_version: "dev-build".to_string(),
                tracked: true,
                installed_version: Some("nightly".to_string()),
                disk_state: "matches-source".to_string(),
                force: false,
            },
        ),
    ]
}

fn version_guard_inputs() -> Vec<(&'static str, &'static str, VersionGuardInput)> {
    let mut inputs = version_guard_semver_install_inputs();
    inputs.extend(version_guard_semver_newer_inputs());
    inputs.extend(version_guard_non_semver_inputs());
    inputs
}

fn generate_version_guard_fixtures(out: &Path) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;
    let mut cases = Vec::new();
    for (name, description, input) in version_guard_inputs() {
        let expected = observe_version_guard(
            &input.bundled_version,
            input.tracked,
            input.installed_version.as_deref(),
            &input.disk_state,
            input.force,
        )?;
        cases.push(VersionGuardCase {
            name: name.to_string(),
            description: description.to_string(),
            input,
            expected,
        });
    }

    let manifest = VersionGuardManifest {
        schema_version: 1,
        cases,
    };
    write_json(&out.join("manifest.json"), &manifest)
}

#[derive(Serialize)]
struct PathsManifest {
    schema_version: u32,
    cases: Vec<PathCase>,
}

#[derive(Serialize)]
struct PathCase {
    name: String,
    input: PathInput,
    expected: PathExpected,
}

#[derive(Serialize)]
struct PathInput {
    platform: String,
    kind: String,
    scope: String,
}

#[derive(Serialize)]
struct PathExpected {
    subpath: String,
    lockname: String,
}

fn generate_paths_fixtures(out: &Path) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;

    let mut cases = Vec::new();
    for scope in InstallScope::ALL {
        for platform in Platform::ALL {
            let paths = crate::paths::ConfigPaths::for_test_with_platform(
                PathBuf::from("/home/testuser"),
                PathBuf::from("/home/testuser/.config/context-mixer"),
                platform,
            );
            let subpath = platform
                .install_subpath(ArtifactKind::Skill, scope)
                .expect("every platform supports skills");
            let lockname = paths
                .lock_path(scope)
                .file_name()
                .context("lock path has file name")?
                .to_string_lossy()
                .to_string();

            cases.push(PathCase {
                name: format!("{}-{}", platform, scope.label()),
                input: PathInput {
                    platform: platform.to_string(),
                    kind: "skill".to_string(),
                    scope: scope.label().to_string(),
                },
                expected: PathExpected {
                    subpath: normalized_path_string(&subpath),
                    lockname,
                },
            });
        }
    }

    let manifest = PathsManifest {
        schema_version: 1,
        cases,
    };
    write_json(&out.join("manifest.json"), &manifest)
}

#[derive(Serialize)]
struct TargetResolveManifest {
    schema_version: u32,
    cases: Vec<TargetResolveCase>,
}

#[derive(Serialize)]
struct TargetResolveCase {
    name: String,
    description: String,
    input: TargetResolveInput,
    expected: TargetResolveExpected,
}

#[derive(Serialize)]
struct TargetResolveInput {
    scope: String,
    config_platforms: Vec<String>,
    non_empty_locks: Vec<String>,
}

#[derive(Serialize)]
struct TargetResolveExpected {
    resolved_platforms: Vec<String>,
}

fn observe_target_resolution(
    config_platforms: &[Platform],
    non_empty_locks: &[Platform],
) -> Result<Vec<String>> {
    let test = TestContext::at(fixed_time());
    if !config_platforms.is_empty() {
        let cfg = CmxConfig {
            platforms: config_platforms.to_vec(),
            ..Default::default()
        };
        config::save_config(&cfg, &test.fs, &test.paths)?;
    }

    for platform in non_empty_locks {
        let mut packages = BTreeMap::new();
        packages.insert(
            "existing-skill".to_string(),
            bundled_lock_entry(Some("1.0.0"), "sha256:existing"),
        );
        let lock = LockFile {
            version: 1,
            packages,
        };
        let paths = test.paths.with_platform(*platform);
        lockfile::save(&lock, InstallScope::Global, &test.fs, &paths)?;
    }

    let resolved =
        targets::resolve_targets(None, ArtifactKind::Skill, InstallScope::Global, &test.ctx())?;
    Ok(resolved.into_iter().map(|platform| platform.to_string()).collect())
}

fn generate_target_resolve_fixtures(out: &Path) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;

    let fixtures = vec![
        (
            "fresh-machine",
            "With no managed set and no non-empty locks, installs target Claude only.",
            vec![],
            vec![],
        ),
        (
            "explicit-config-set",
            "A non-empty managed config set overrides inferred locks.",
            vec![Platform::Codex, Platform::Gemini],
            vec![Platform::Claude, Platform::Hermes],
        ),
        (
            "unmanaged-nonempty-lock-inference",
            "Without a managed config, installs target every platform whose lockfile is already non-empty.",
            vec![],
            vec![Platform::Codex, Platform::Hermes],
        ),
    ];

    let mut cases = Vec::new();
    for (name, description, config_platforms, non_empty_locks) in fixtures {
        let expected = observe_target_resolution(&config_platforms, &non_empty_locks)?;
        cases.push(TargetResolveCase {
            name: name.to_string(),
            description: description.to_string(),
            input: TargetResolveInput {
                scope: InstallScope::Global.label().to_string(),
                config_platforms: config_platforms
                    .into_iter()
                    .map(|platform| platform.to_string())
                    .collect(),
                non_empty_locks: non_empty_locks
                    .into_iter()
                    .map(|platform| platform.to_string())
                    .collect(),
            },
            expected: TargetResolveExpected {
                resolved_platforms: expected,
            },
        });
    }

    let manifest = TargetResolveManifest {
        schema_version: 1,
        cases,
    };
    write_json(&out.join("manifest.json"), &manifest)
}

#[derive(Serialize)]
struct InstallE2eManifest {
    schema_version: u32,
    cases: Vec<InstallE2eCase>,
}

#[derive(Serialize)]
struct InstallE2eCase {
    name: String,
    description: String,
    input: InstallE2eInput,
    expected: InstallE2eExpectedPaths,
}

#[derive(Serialize)]
struct InstallE2eInput {
    tool_name: String,
    tool_version: String,
    scope: String,
    force: bool,
    bundle_dir: String,
    pre_tree_dir: String,
    pre_locks_dir: String,
}

#[derive(Serialize)]
struct InstallE2eExpectedPaths {
    tree_dir: String,
    locks_dir: String,
    report_path: String,
}

#[derive(Serialize)]
struct PlanSnapshot {
    blocked: bool,
    cmx_present: bool,
    scope: String,
    source_checksum: String,
    targets: Vec<PlanTargetSnapshot>,
}

#[derive(Serialize)]
struct PlanTargetSnapshot {
    platform: String,
    dest_dir: String,
    action: ActionSnapshot,
    cmx_managed: bool,
}

#[derive(Serialize)]
struct ReportSnapshot {
    tool_name: String,
    scope: String,
    source_registered: bool,
    targets: Vec<ReportTargetSnapshot>,
}

#[derive(Serialize)]
struct ReportTargetSnapshot {
    platform: String,
    dest_dir: String,
    action: ActionSnapshot,
    files_written: usize,
    installed_checksum: Option<String>,
    discarded_paths: Vec<String>,
}

#[derive(Serialize)]
struct ApplySnapshot {
    status: String,
    error: Option<String>,
    report: Option<ReportSnapshot>,
}

#[derive(Serialize)]
struct InstallE2eReport {
    plan: PlanSnapshot,
    apply: ApplySnapshot,
}

fn snapshot_plan(plan: &InstallPlan) -> PlanSnapshot {
    PlanSnapshot {
        blocked: plan.is_blocked(),
        cmx_present: plan.cmx_present,
        scope: plan.scope.label().to_string(),
        source_checksum: plan.source_checksum.clone(),
        targets: plan
            .targets
            .iter()
            .map(|target| PlanTargetSnapshot {
                platform: target.platform.to_string(),
                dest_dir: normalized_path_string(&target.dest_dir),
                action: snapshot_action(&target.action),
                cmx_managed: target.cmx_managed,
            })
            .collect(),
    }
}

fn snapshot_report(report: &Report) -> ReportSnapshot {
    ReportSnapshot {
        tool_name: report.tool.name.clone(),
        scope: report.scope.label().to_string(),
        source_registered: report.source_registered,
        targets: report
            .targets
            .iter()
            .map(|target| ReportTargetSnapshot {
                platform: target.platform.to_string(),
                dest_dir: normalized_path_string(&target.dest_dir),
                action: snapshot_action(&target.action),
                files_written: target.files_written,
                installed_checksum: target.installed_checksum.clone(),
                discarded_paths: target
                    .discarded_paths
                    .iter()
                    .map(|path| normalized_path_string(path))
                    .collect(),
            })
            .collect(),
    }
}

struct InstallE2eFixture<'a> {
    name: &'a str,
    description: &'a str,
    tool_version: &'a str,
    force: bool,
    setup: fn(&TestContext, &BundledSkill) -> Result<()>,
}

fn setup_fresh_install(test: &TestContext, _skill: &BundledSkill) -> Result<()> {
    let _ = snapshot_locks(test, InstallScope::Global)?;
    Ok(())
}

fn setup_skip_identical(test: &TestContext, skill: &BundledSkill) -> Result<()> {
    let checksum = write_bundle_version(test, InstallScope::Global, skill, FIXTURE_VERSION)?;
    save_bundled_lock(test, InstallScope::Global, Some(FIXTURE_VERSION), &checksum)
}

fn setup_drifted_skip(test: &TestContext, skill: &BundledSkill) -> Result<()> {
    let checksum = bundled_checksum(skill, FIXTURE_VERSION);
    save_bundled_lock(test, InstallScope::Global, Some(FIXTURE_VERSION), &checksum)?;
    let _ = write_drifted_bundle_version(test, InstallScope::Global, skill, FIXTURE_VERSION)?;
    Ok(())
}

fn setup_update_from_older(test: &TestContext, skill: &BundledSkill) -> Result<()> {
    let checksum = write_bundle_version(test, InstallScope::Global, skill, "1.0.0")?;
    save_bundled_lock(test, InstallScope::Global, Some("1.0.0"), &checksum)
}

fn setup_refuse_newer(test: &TestContext, skill: &BundledSkill) -> Result<()> {
    let checksum = write_bundle_version(test, InstallScope::Global, skill, "9.0.0")?;
    save_bundled_lock(test, InstallScope::Global, Some("9.0.0"), &checksum)
}

fn run_install_e2e_case(root: &Path, fixture: &InstallE2eFixture<'_>) -> Result<InstallE2eCase> {
    let test = TestContext::at(fixed_time());
    let skill = bundle();

    (fixture.setup)(&test, &skill)?;

    let pre_tree = snapshot_tree(&test, InstallScope::Global);
    let pre_locks = snapshot_locks(&test, InstallScope::Global)?;

    let case_dir = root.join(fixture.name);
    write_tree_snapshot(&case_dir.join("bundle"), &bundle_snapshot(&skill))?;
    write_tree_snapshot(&case_dir.join("pre").join("tree"), &pre_tree)?;
    write_lock_snapshot(&case_dir.join("pre").join("locks"), &pre_locks)?;

    let installer = installer(fixture.tool_version);
    let plan = installer.plan(&skill, Scope::Global, fixture.force, &test.ctx())?;
    let plan_snapshot = snapshot_plan(&plan);

    let apply = match installer.apply(&skill, &plan, &test.ctx()) {
        Ok(report) => ApplySnapshot {
            status: "applied".to_string(),
            error: None,
            report: Some(snapshot_report(&report)),
        },
        Err(error) => ApplySnapshot {
            status: "blocked".to_string(),
            error: Some(error.to_string()),
            report: None,
        },
    };

    let expected_tree = snapshot_tree(&test, InstallScope::Global);
    let expected_locks = snapshot_locks(&test, InstallScope::Global)?;

    write_tree_snapshot(&case_dir.join("expected").join("tree"), &expected_tree)?;
    write_lock_snapshot(&case_dir.join("expected").join("locks"), &expected_locks)?;
    write_json(
        &case_dir.join("expected").join("report.json"),
        &InstallE2eReport {
            plan: plan_snapshot,
            apply,
        },
    )?;

    Ok(InstallE2eCase {
        name: fixture.name.to_string(),
        description: fixture.description.to_string(),
        input: InstallE2eInput {
            tool_name: FIXTURE_TOOL_NAME.to_string(),
            tool_version: fixture.tool_version.to_string(),
            scope: InstallScope::Global.label().to_string(),
            force: fixture.force,
            bundle_dir: format!("{}/bundle", fixture.name),
            pre_tree_dir: format!("{}/pre/tree", fixture.name),
            pre_locks_dir: format!("{}/pre/locks", fixture.name),
        },
        expected: InstallE2eExpectedPaths {
            tree_dir: format!("{}/expected/tree", fixture.name),
            locks_dir: format!("{}/expected/locks", fixture.name),
            report_path: format!("{}/expected/report.json", fixture.name),
        },
    })
}

fn bundle_snapshot(skill: &BundledSkill) -> BTreeMap<PathBuf, Vec<u8>> {
    skill
        .files
        .iter()
        .map(|file| (file.rel_path.clone(), file.bytes.clone()))
        .collect()
}

fn generate_install_e2e_fixtures(out: &Path) -> Result<()> {
    fs::create_dir_all(out).with_context(|| format!("create {}", out.display()))?;

    let fixtures = vec![
        InstallE2eFixture {
            name: "fresh-install",
            description: "Fresh unmanaged install onto a machine with no existing locks.",
            tool_version: FIXTURE_VERSION,
            force: false,
            setup: setup_fresh_install,
        },
        InstallE2eFixture {
            name: "skip-identical",
            description: "Tracked current install with matching bytes skips cleanly.",
            tool_version: FIXTURE_VERSION,
            force: false,
            setup: setup_skip_identical,
        },
        InstallE2eFixture {
            name: "drifted-skip",
            description: "Tracked current install with local edits preserves those edits when force is false.",
            tool_version: FIXTURE_VERSION,
            force: false,
            setup: setup_drifted_skip,
        },
        InstallE2eFixture {
            name: "update-from-older",
            description: "Tracked older install updates to the bundled version.",
            tool_version: FIXTURE_VERSION,
            force: false,
            setup: setup_update_from_older,
        },
        InstallE2eFixture {
            name: "refuse-newer",
            description: "Tracked newer install blocks the plan without force.",
            tool_version: FIXTURE_VERSION,
            force: false,
            setup: setup_refuse_newer,
        },
        InstallE2eFixture {
            name: "force-downgrade",
            description: "Tracked newer install downgrades when force is true.",
            tool_version: FIXTURE_VERSION,
            force: true,
            setup: setup_refuse_newer,
        },
    ];

    let mut cases = Vec::new();
    for fixture in &fixtures {
        cases.push(run_install_e2e_case(out, fixture)?);
    }

    let manifest = InstallE2eManifest {
        schema_version: 1,
        cases,
    };
    write_json(&out.join("manifest.json"), &manifest)
}

fn write_readme(path: &Path) -> Result<()> {
    write_bytes(path, README.as_bytes())
}

const README: &str = r#"# cmx-core conformance fixtures

These fixtures are the language-neutral correctness contract for `cmx-core` ports.
Every case is generated from the Rust reference implementation with:

```bash
cargo run -p cmx-core --features test-support --bin generate-conformance-fixtures
```

The generator is a dedicated `test-support` binary so regeneration is explicit and reproducible, while the drift-guard test reuses the same library function in CI.

## Fixed environment

- Fixed clock: `2026-07-05T12:00:00+00:00`
- Virtual home: `/home/testuser`
- Global config root: `/home/testuser/.config/context-mixer`
- Local project root mapping: relative install paths are stored under `project/` in fixture trees

Tree snapshots use real files on disk. Absolute paths are stored without the leading `/`, so `/home/testuser/.claude/skills/fixture-tool/SKILL.md` appears as `home/testuser/.claude/skills/fixture-tool/SKILL.md`.

Lockfile expectations are stored as parsed JSON values, not byte-for-byte serialized text. Future ports must compare lockfiles as JSON values with sorted package keys.

## Layout

```text
cmx-core/conformance/
  README.md
  checksum/
  frontmatter/
  version-guard/
  paths/
  target-resolve/
  install-e2e/
```

Each category has one `manifest.json` that defines the schema for its cases. Inputs and expected outputs are separated explicitly.

## Category schemas

### `checksum/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "string-sort-a-dot-slash",
      "description": "human-readable note",
      "input": {
        "files": [
          { "path": "a", "content_utf8": "bare\n" }
        ]
      },
      "expected": {
        "sha256": "sha256:...",
        "canonical_order": ["a", "a.b", "a/b"],
        "canonical_included_paths": ["a", "a.b", "a/b"]
      }
    }
  ]
}
```

Notes:

- Checksum cases use inline UTF-8 file sets because some parity cases, such as `a` plus `a/b`, cannot exist simultaneously on a real filesystem tree.
- `canonical_order` and `canonical_included_paths` are the reference's filtered, sorted input to the hash.

### `frontmatter/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "existing-metadata-version",
      "description": "human-readable note",
      "input": {
        "version": "2.4.6",
        "skill_md_path": "existing-metadata-version/input/SKILL.md"
      },
      "expected": {
        "skill_md_path": "existing-metadata-version/expected/SKILL.md",
        "byte_exact": true,
        "idempotent_second_pass": false
      }
    }
  ]
}
```

The `input/` and `expected/` files are real `SKILL.md` byte fixtures. Ports must compare the expected output byte-for-byte.

### `version-guard/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "equal-drifted-skip",
      "description": "human-readable note",
      "input": {
        "bundled_version": "2.4.6",
        "tracked": true,
        "installed_version": "2.4.6",
        "disk_state": "drifted",
        "force": false
      },
      "expected": {
        "kind": "drifted-skip",
        "from": null,
        "installed": "2.4.6",
        "will_write": false,
        "blocked": false
      }
    }
  ]
}
```

`disk_state` is one of `missing`, `matches-source`, or `drifted`.

### `paths/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "copilot-global",
      "input": {
        "platform": "copilot",
        "kind": "skill",
        "scope": "global"
      },
      "expected": {
        "subpath": ".copilot/skills",
        "lockname": "cmx-lock-copilot.json"
      }
    }
  ]
}
```

This category covers every platform in `Platform::ALL` at both scopes.

### `target-resolve/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "fresh-machine",
      "description": "human-readable note",
      "input": {
        "scope": "global",
        "config_platforms": [],
        "non_empty_locks": []
      },
      "expected": {
        "resolved_platforms": ["claude"]
      }
    }
  ]
}
```

`non_empty_locks` lists the platforms whose scope-specific lockfiles were pre-populated before resolution.

### `install-e2e/manifest.json`

Schema:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "name": "fresh-install",
      "description": "human-readable note",
      "input": {
        "tool_name": "fixture-tool",
        "tool_version": "2.4.6",
        "scope": "global",
        "force": false,
        "bundle_dir": "fresh-install/bundle",
        "pre_tree_dir": "fresh-install/pre/tree",
        "pre_locks_dir": "fresh-install/pre/locks"
      },
      "expected": {
        "tree_dir": "fresh-install/expected/tree",
        "locks_dir": "fresh-install/expected/locks",
        "report_path": "fresh-install/expected/report.json"
      }
    }
  ]
}
```

Case contents:

- `bundle/` is the original bundled skill file set before frontmatter reconciliation.
- `pre/tree/` is the non-lock virtual filesystem tree before `plan`/`apply`.
- `pre/locks/` stores any pre-existing lockfiles as JSON values, keyed by lock filename.
- `expected/tree/` and `expected/locks/` are the post-apply filesystem state.
- `expected/report.json` contains:
  - `plan`: the observed plan snapshot from the Rust oracle
  - `apply.status`: `applied` or `blocked`
  - `apply.error`: present only for blocked runs
  - `apply.report`: the normalized Rust `Report` snapshot for successful applies

Ports should materialize `bundle/`, `pre/tree/`, and `pre/locks/` into an isolated test root, execute the equivalent operation, then compare the resulting tree, lock JSON values, and normalized report against `expected/`.

## Drift guard

`cargo test --workspace` includes a drift-guard test that regenerates this entire tree into a temp directory and compares it against the committed fixtures. JSON files are compared by parsed value; all other files are compared byte-for-byte.
"#;

#[cfg(test)]
struct DiskTree {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

#[cfg(test)]
fn collect_disk_tree(root: &Path) -> Result<DiskTree> {
    let mut tree = DiskTree {
        files: BTreeMap::new(),
    };
    collect_disk_tree_recursive(root, root, &mut tree)?;
    Ok(tree)
}

// Records files only — empty directories are intentionally ignored (see
// `assert_fixture_tree_matches`: git cannot track them, so they are not part of
// the fixture contract).
#[cfg(test)]
fn collect_disk_tree_recursive(root: &Path, current: &Path, tree: &mut DiskTree) -> Result<()> {
    for entry in fs::read_dir(current).with_context(|| format!("read {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .with_context(|| format!("strip {} from {}", root.display(), path.display()))?
            .to_path_buf();

        if entry.file_type()?.is_dir() {
            collect_disk_tree_recursive(root, &path, tree)?;
        } else {
            let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            tree.files.insert(rel, bytes);
        }
    }
    Ok(())
}

#[cfg(test)]
fn assert_fixture_tree_matches(expected_root: &Path, actual_root: &Path) -> Result<()> {
    let expected = collect_disk_tree(expected_root)?;
    let actual = collect_disk_tree(actual_root)?;

    // Deliberately do NOT compare the empty-directory set. Git cannot track
    // empty directories, so any purely-empty scaffold dir the generator emits
    // (e.g. a fresh-install case's empty `pre/tree/...`) survives in a freshly
    // regenerated tree but vanishes from the committed tree — making a strict
    // `dirs ==` check pass only in the dirty worktree that just generated them
    // and fail from every clean checkout. The contract is the set of files and
    // their contents; the file checks below are authoritative and catch any
    // meaningful drift (a non-empty dir necessarily shows up as a file path).
    ensure!(
        expected.files.keys().collect::<Vec<_>>() == actual.files.keys().collect::<Vec<_>>(),
        "file set drift:\nexpected: {:?}\nactual: {:?}",
        expected.files.keys().collect::<Vec<_>>(),
        actual.files.keys().collect::<Vec<_>>()
    );

    for (rel_path, expected_bytes) in expected.files {
        let actual_bytes = actual
            .files
            .get(&rel_path)
            .with_context(|| format!("missing generated file {}", rel_path.display()))?;
        if rel_path.extension().is_some_and(|ext| ext == "json") {
            let expected_value = serde_json::from_slice::<Value>(&expected_bytes)
                .with_context(|| format!("parse committed JSON {}", rel_path.display()))?;
            let actual_value = serde_json::from_slice::<Value>(actual_bytes)
                .with_context(|| format!("parse regenerated JSON {}", rel_path.display()))?;
            ensure!(expected_value == actual_value, "JSON drift in {}", rel_path.display());
        } else {
            ensure!(expected_bytes == *actual_bytes, "byte drift in {}", rel_path.display());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_conformance_fixtures_match_regeneration() {
        let tmp = tempfile::tempdir().unwrap();
        let regenerated = tmp.path().join("conformance");
        generate_conformance_fixtures(&regenerated).unwrap();

        let committed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance");
        assert_fixture_tree_matches(&committed, &regenerated).unwrap();
    }
}
