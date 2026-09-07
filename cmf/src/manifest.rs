//! The compile manifest: cmf's second output artifact, recording which intent
//! records were composed into a delivered artifact so a verifier can later hold
//! the project to exactly those intents (see `CMV.md`, "The manifest").
//!
//! The manifest is machine-written JSON with a `schema` version. Every effect —
//! reading record bytes, the clock, the git `HEAD`, and the cmx sources
//! registry — goes through the [`cmx_core::context::AppContext`] gateways, so
//! the same code produces byte-identical output against the in-memory fakes.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use cmx_core::checksum;
use cmx_core::config;
use cmx_core::context::AppContext;
use cmx_core::gateway::Filesystem;
use cmx_core::json_file;
use cmx_core::paths::ConfigPaths;
use cmx_core::types::{InstallScope, SourcesFile};
use serde::{Deserialize, Serialize};

use crate::assembly::Assembly;
use crate::catalog::Intent;
use crate::profile::{Profile, Surface};

/// Manifest schema version written in the `schema` field.
pub const SCHEMA_VERSION: u32 = 1;

/// File name of the manifest `cmf install --local --apply` writes beside the
/// local lock file.
pub const LOCAL_MANIFEST_FILE_NAME: &str = "cmf-manifest.json";

/// Everything a verifier needs to know about one compiled artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest schema version; always [`SCHEMA_VERSION`] for manifests this
    /// crate writes.
    pub schema: u32,
    /// RFC 3339 instant the artifact was compiled, from the `Clock` gateway.
    pub compiled_at: String,
    /// Where the intent records were read from.
    pub knowledge_base: KnowledgeBase,
    /// The profile that drove selection.
    pub profile: ProfileRef,
    /// The delivered artifact.
    pub artifact: ArtifactRef,
    /// The intents retained in the artifact, in `Assembly.selected` order.
    pub intents: Vec<IntentRef>,
    /// Intents the profile asked for that did not survive assembly. Always
    /// present; empty until assembly learns to drop by budget instead of
    /// failing.
    pub dropped: Vec<DroppedIntent>,
}

/// The knowledge base a manifest was compiled from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBase {
    /// The cmx source name, when the root matches a registered source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The knowledge-base root as given to cmf.
    pub path: PathBuf,
    /// The git `HEAD` commit, when the root is a git checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

/// Identity of the profile that drove selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileRef {
    /// Stable profile id.
    pub id: String,
    /// Artifact version the profile declares.
    pub version: String,
    /// Ecosystems the profile declared in `[select] ecosystems`, in profile
    /// order; empty when it declared none and so applied no filter. Always
    /// written, so a verifier can compare it with the languages it detects.
    /// Read with a default so manifests written before the field existed
    /// still load.
    #[serde(default)]
    pub ecosystems: Vec<String>,
}

/// Identity and integrity of the delivered artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    /// Installed artifact name.
    pub name: String,
    /// Delivery surface.
    pub surface: Surface,
    /// `sha256:<hex>` of the rendered artifact content.
    pub checksum: String,
}

/// One intent record retained in the artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentRef {
    /// The record's stable `id` field.
    pub id: String,
    /// The path-derived catalog key.
    pub key: String,
    /// `sha256:<hex>` of the record file's bytes.
    pub checksum: String,
}

/// One intent the profile asked for that assembly did not deliver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroppedIntent {
    /// The path-derived catalog key.
    pub key: String,
    /// Why it was dropped, e.g. `budget`.
    pub reason: String,
}

/// Build the manifest for one assembly.
///
/// `root` is the knowledge-base root exactly as the caller resolved it; it is
/// recorded verbatim in `knowledge_base.path` and used to look up the git
/// revision and the registered cmx source.
pub fn build(
    root: &Path,
    profile: &Profile,
    assembly: &Assembly,
    intents: &BTreeMap<String, Intent>,
    ctx: &AppContext<'_>,
) -> Result<Manifest> {
    let sources = config::load_sources(ctx.fs, ctx.paths)?;
    Ok(Manifest {
        schema: SCHEMA_VERSION,
        compiled_at: ctx.clock.now().to_rfc3339(),
        knowledge_base: KnowledgeBase {
            source: registered_source_name(root, &sources, ctx.fs),
            path: root.to_path_buf(),
            revision: git_head_commit(root, ctx.fs),
        },
        profile: ProfileRef {
            id: profile.id.clone(),
            version: profile.version.clone(),
            ecosystems: profile.select.ecosystems.clone(),
        },
        artifact: ArtifactRef {
            name: profile.artifact_name().to_string(),
            surface: profile.surface,
            checksum: checksum::checksum_bytes(assembly.content.as_bytes()),
        },
        intents: intent_entries(&assembly.selected, intents, ctx.fs)?,
        dropped: Vec::new(),
    })
}

/// Resolve each selected key against the catalog and checksum its record file.
///
/// Order follows `selected`. A key missing from the catalog is an internal
/// invariant violation (assembly only selects catalog keys) and is an error.
pub fn intent_entries(
    selected: &[String],
    intents: &BTreeMap<String, Intent>,
    fs: &dyn Filesystem,
) -> Result<Vec<IntentRef>> {
    selected
        .iter()
        .map(|key| {
            let Some(intent) = intents.get(key) else {
                bail!("selected intent {key:?} is not in the scanned catalog");
            };
            let checksum = checksum::checksum_file(&intent.path, fs)
                .with_context(|| format!("could not checksum intent {}", intent.path.display()))?;
            Ok(IntentRef {
                id: intent.record.id.clone(),
                key: key.clone(),
                checksum,
            })
        })
        .collect()
}

/// The name of the registered cmx source whose directory is `root`, if any.
///
/// A source matches when its `path` (local) or `local_clone` (git) resolves to
/// the same canonical directory as `root`. Sources whose directory cannot be
/// canonicalized (for example, not yet cloned) are skipped. Ties resolve to
/// the first name in sorted order.
pub fn registered_source_name(
    root: &Path,
    sources: &SourcesFile,
    fs: &dyn Filesystem,
) -> Option<String> {
    let root = fs.canonicalize(root).ok()?;
    sources
        .sources
        .iter()
        .find(|(_, entry)| {
            config::resolve_local_path(entry)
                .ok()
                .and_then(|path| fs.canonicalize(&path).ok())
                .is_some_and(|path| path == root)
        })
        .map(|(name, _)| name.clone())
}

/// The commit `HEAD` points at when `root` is a git checkout, else `None`.
///
/// Reads the repository metadata directly: `.git` may be the directory itself
/// or a `gitdir:` pointer file (worktrees, submodules). A symbolic `HEAD` is
/// followed to its loose ref and then to `packed-refs`, in the common
/// directory when the checkout is a worktree. Anything unreadable or
/// unrecognized yields `None`, so an unavailable revision is omitted rather
/// than guessed.
pub fn git_head_commit(root: &Path, fs: &dyn Filesystem) -> Option<String> {
    let git_dir = git_dir(root, fs)?;
    let head = fs.read_to_string(&git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(reference) => {
            let common_dir = fs
                .read_to_string(&git_dir.join("commondir"))
                .ok()
                .map_or(git_dir.clone(), |common| lexical_join(&git_dir, common.trim()));
            resolve_ref(reference.trim(), &common_dir, fs)
        }
        None => commit_hash(head),
    }
}

/// Locate the git directory for `root`: `.git` itself, or the target of a
/// `gitdir:` pointer file.
fn git_dir(root: &Path, fs: &dyn Filesystem) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if fs.is_dir(&dot_git) {
        return Some(dot_git);
    }
    let pointer = fs.read_to_string(&dot_git).ok()?;
    let target = pointer.trim().strip_prefix("gitdir:")?.trim();
    Some(lexical_join(root, target))
}

/// Resolve a fully qualified ref (`refs/heads/main`) to a commit hash via its
/// loose file, falling back to `packed-refs`.
fn resolve_ref(reference: &str, common_dir: &Path, fs: &dyn Filesystem) -> Option<String> {
    if let Ok(loose) = fs.read_to_string(&lexical_join(common_dir, reference)) {
        return commit_hash(loose.trim());
    }
    let packed = fs.read_to_string(&common_dir.join("packed-refs")).ok()?;
    packed
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with('^'))
        .filter_map(|line| line.split_once(' '))
        .find(|(_, name)| name.trim() == reference)
        .and_then(|(hash, _)| commit_hash(hash))
}

/// Accept `value` as a commit hash only when it is plausibly one.
fn commit_hash(value: &str) -> Option<String> {
    let is_hash = value.len() >= 40 && value.chars().all(|c| c.is_ascii_hexdigit());
    is_hash.then(|| value.to_string())
}

/// Join `relative` onto `base`, collapsing `.` and `..` lexically so the
/// result is a single canonical key for gateway lookups (git writes
/// `commondir` entries like `../..`).
fn lexical_join(base: &Path, relative: &str) -> PathBuf {
    let relative = Path::new(relative);
    if relative.is_absolute() {
        return relative.to_path_buf();
    }
    let mut joined = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::ParentDir => {
                joined.pop();
            }
            Component::CurDir => {}
            other => joined.push(other),
        }
    }
    joined
}

/// Where `cmf install --local` writes the manifest: beside the local lock file.
pub fn local_manifest_path(paths: &ConfigPaths) -> PathBuf {
    paths.lock_path(InstallScope::Local).with_file_name(LOCAL_MANIFEST_FILE_NAME)
}

impl Manifest {
    /// Render as pretty-printed JSON with a trailing newline.
    pub fn to_json(&self) -> Result<String> {
        let mut json =
            serde_json::to_string_pretty(self).context("could not serialize manifest")?;
        json.push('\n');
        Ok(json)
    }
}

/// Write `manifest` to `path` atomically (sibling temp file, then rename),
/// creating parent directories as needed.
pub fn write(manifest: &Manifest, path: &Path, fs: &dyn Filesystem) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs.create_dir_all(parent)?;
    }
    let tmp = json_file::tmp_path(path);
    fs.write(&tmp, &manifest.to_json()?)?;
    fs.rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::IntentRecord;
    use crate::profile::{Content, Graph, Selection};
    use chrono::{TimeZone, Utc};
    use cmx_core::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
    use cmx_core::test_support::{make_git_entry, make_local_entry};

    const HASH: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    const OTHER_HASH: &str = "ffffffffffffffffffffffffffffffffffffffff";

    fn paths() -> ConfigPaths {
        ConfigPaths::for_test("/home/tester".into(), "/home/tester/.config/context-mixer".into())
    }

    fn intent(root: &Path, key: &str, body: &str) -> (String, Intent) {
        let path = root.join("intents").join(format!("{key}.toml"));
        (
            key.to_string(),
            Intent {
                key: key.to_string(),
                path,
                record: IntentRecord {
                    id: format!("kb.intent.{key}"),
                    title: key.to_string(),
                    category: "quality".to_string(),
                    tags: vec![],
                    status: "confirmed".to_string(),
                    capability: body.to_string(),
                    threat: String::new(),
                    expectation: String::new(),
                    strategy: String::new(),
                    tradeoff: String::new(),
                    relations: vec![],
                    evidence: vec![],
                },
            },
        )
    }

    fn profile() -> Profile {
        Profile {
            id: "shipping".to_string(),
            name: Some("AGENTS".to_string()),
            version: "0.2.0".to_string(),
            description: "Ship it".to_string(),
            surface: Surface::Agent,
            budget_tokens: 100,
            select: Selection {
                ecosystems: vec!["rust".to_string()],
                ..Selection::default()
            },
            graph: Graph::default(),
            content: Content::default(),
        }
    }

    fn assembly(selected: &[&str]) -> Assembly {
        Assembly {
            content: "rendered guidance\n".to_string(),
            selected: selected.iter().map(ToString::to_string).collect(),
            traversed: vec![],
            excluded_by_ecosystem: 0,
            estimated_tokens: 5,
        }
    }

    fn catalog(root: &Path, fs: &FakeFilesystem) -> BTreeMap<String, Intent> {
        let entries = [
            intent(root, "b/second", "two"),
            intent(root, "a/first", "one"),
        ];
        for (_, intent) in &entries {
            fs.add_file(intent.path.clone(), intent.record.capability.as_bytes());
        }
        entries.into_iter().collect()
    }

    fn build_with(root: &Path, fs: &FakeFilesystem, selected: &[&str]) -> Manifest {
        let intents = catalog(root, fs);
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc.with_ymd_and_hms(2026, 9, 5, 14, 2, 11).unwrap());
        let paths = paths();
        let ctx = AppContext {
            fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: None,
        };
        build(root, &profile(), &assembly(selected), &intents, &ctx).unwrap()
    }

    #[test]
    fn records_profile_artifact_and_clock() {
        let fs = FakeFilesystem::new();
        let root = Path::new("/kb");
        let manifest = build_with(root, &fs, &["a/first"]);
        assert_eq!(manifest.schema, SCHEMA_VERSION);
        assert_eq!(manifest.compiled_at, "2026-09-05T14:02:11+00:00");
        assert_eq!(manifest.knowledge_base.path, PathBuf::from("/kb"));
        assert_eq!(manifest.profile.id, "shipping");
        assert_eq!(manifest.profile.version, "0.2.0");
        assert_eq!(manifest.profile.ecosystems, ["rust"]);
        assert_eq!(manifest.artifact.name, "AGENTS");
        assert_eq!(manifest.artifact.surface, Surface::Agent);
        assert_eq!(
            manifest.artifact.checksum,
            checksum::checksum_bytes(b"rendered guidance\n"),
            "artifact checksum is cmx-core's checksum of the rendered content"
        );
        assert!(manifest.dropped.is_empty());
    }

    #[test]
    fn intents_follow_selected_order_with_record_checksums() {
        let fs = FakeFilesystem::new();
        let root = Path::new("/kb");
        let manifest = build_with(root, &fs, &["b/second", "a/first"]);
        let keys: Vec<_> = manifest.intents.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["b/second", "a/first"], "order is Assembly.selected, not catalog order");
        assert_eq!(manifest.intents[0].id, "kb.intent.b/second");
        assert_eq!(manifest.intents[0].checksum, checksum::checksum_bytes(b"two"));
        assert_eq!(manifest.intents[1].checksum, checksum::checksum_bytes(b"one"));
    }

    #[test]
    fn selected_key_missing_from_catalog_is_an_error() {
        let fs = FakeFilesystem::new();
        let root = Path::new("/kb");
        let intents = catalog(root, &fs);
        let err = intent_entries(&["ghost".to_string()], &intents, &fs).unwrap_err();
        assert!(err.to_string().contains("ghost"), "unexpected: {err}");
    }

    #[test]
    fn revision_omitted_when_root_is_not_a_git_checkout() {
        let fs = FakeFilesystem::new();
        let manifest = build_with(Path::new("/kb"), &fs, &["a/first"]);
        assert_eq!(manifest.knowledge_base.revision, None);
        let json = manifest.to_json().unwrap();
        assert!(!json.contains("revision"), "absent revision must be omitted, not null:\n{json}");
    }

    #[test]
    fn revision_follows_symbolic_head_to_loose_ref() {
        let fs = FakeFilesystem::new();
        fs.add_file("/kb/.git/HEAD", "ref: refs/heads/main\n");
        fs.add_file("/kb/.git/refs/heads/main", format!("{HASH}\n"));
        assert_eq!(git_head_commit(Path::new("/kb"), &fs), Some(HASH.to_string()));
    }

    #[test]
    fn revision_falls_back_to_packed_refs() {
        let fs = FakeFilesystem::new();
        fs.add_file("/kb/.git/HEAD", "ref: refs/heads/main\n");
        fs.add_file(
            "/kb/.git/packed-refs",
            format!(
                "# pack-refs with: peeled fully-peeled sorted\n{OTHER_HASH} refs/heads/other\n{HASH} refs/heads/main\n^{OTHER_HASH}\n"
            ),
        );
        assert_eq!(git_head_commit(Path::new("/kb"), &fs), Some(HASH.to_string()));
    }

    #[test]
    fn revision_reads_detached_head_directly() {
        let fs = FakeFilesystem::new();
        fs.add_file("/kb/.git/HEAD", format!("{HASH}\n"));
        assert_eq!(git_head_commit(Path::new("/kb"), &fs), Some(HASH.to_string()));
    }

    #[test]
    fn revision_follows_gitdir_pointer_and_commondir_for_worktrees() {
        let fs = FakeFilesystem::new();
        fs.add_file("/wt/.git", "gitdir: /main/.git/worktrees/wt\n");
        fs.add_file("/main/.git/worktrees/wt/HEAD", "ref: refs/heads/feature\n");
        fs.add_file("/main/.git/worktrees/wt/commondir", "../..\n");
        fs.add_file("/main/.git/refs/heads/feature", format!("{HASH}\n"));
        assert_eq!(git_head_commit(Path::new("/wt"), &fs), Some(HASH.to_string()));
    }

    #[test]
    fn revision_omitted_for_unresolvable_or_malformed_head() {
        let fs = FakeFilesystem::new();
        fs.add_file("/dangling/.git/HEAD", "ref: refs/heads/nowhere\n");
        assert_eq!(git_head_commit(Path::new("/dangling"), &fs), None);
        fs.add_file("/garbage/.git/HEAD", "not a hash\n");
        assert_eq!(git_head_commit(Path::new("/garbage"), &fs), None);
    }

    #[test]
    fn source_omitted_when_root_is_not_registered() {
        let fs = FakeFilesystem::new();
        let paths = paths();
        let mut sources = SourcesFile::default();
        sources
            .sources
            .insert("elsewhere".to_string(), make_local_entry("/other", None));
        config::save_sources(&sources, &fs, &paths).unwrap();
        let manifest = build_with(Path::new("/kb"), &fs, &["a/first"]);
        assert_eq!(manifest.knowledge_base.source, None);
        let json = manifest.to_json().unwrap();
        assert!(!json.contains("\"source\""), "absent source must be omitted, not null:\n{json}");
    }

    #[test]
    fn source_named_when_local_source_path_matches_root() {
        let fs = FakeFilesystem::new();
        let paths = paths();
        let mut sources = SourcesFile::default();
        sources.sources.insert("guidelines".to_string(), make_local_entry("/kb", None));
        config::save_sources(&sources, &fs, &paths).unwrap();
        let manifest = build_with(Path::new("/kb"), &fs, &["a/first"]);
        assert_eq!(manifest.knowledge_base.source.as_deref(), Some("guidelines"));
    }

    #[test]
    fn source_named_when_git_source_clone_matches_root() {
        let fs = FakeFilesystem::new();
        let mut sources = SourcesFile::default();
        sources.sources.insert(
            "remote-kb".to_string(),
            make_git_entry("https://example.com/kb.git", "/clones/kb", "main", None),
        );
        assert_eq!(
            registered_source_name(Path::new("/clones/kb"), &sources, &fs).as_deref(),
            Some("remote-kb")
        );
    }

    #[test]
    fn to_json_is_pretty_with_trailing_newline() {
        let fs = FakeFilesystem::new();
        let json = build_with(Path::new("/kb"), &fs, &["a/first"]).to_json().unwrap();
        assert!(json.starts_with("{\n  \"schema\": 1,\n"), "unexpected head:\n{json}");
        assert!(json.ends_with("}\n"), "must end with exactly one newline");
        assert!(!json.ends_with("\n\n"));
    }

    #[test]
    fn json_round_trips() {
        let fs = FakeFilesystem::new();
        fs.add_file("/kb/.git/HEAD", format!("{HASH}\n"));
        let manifest = build_with(Path::new("/kb"), &fs, &["a/first", "b/second"]);
        let parsed: Manifest = serde_json::from_str(&manifest.to_json().unwrap()).unwrap();
        assert_eq!(parsed, manifest);
    }

    #[test]
    fn manifest_written_before_ecosystems_existed_still_loads() {
        let fs = FakeFilesystem::new();
        let mut json: serde_json::Value = serde_json::from_str(
            &build_with(Path::new("/kb"), &fs, &["a/first"]).to_json().unwrap(),
        )
        .unwrap();
        json["profile"].as_object_mut().unwrap().remove("ecosystems");
        let parsed: Manifest = serde_json::from_value(json).unwrap();
        assert!(parsed.profile.ecosystems.is_empty(), "absent field reads as no filter");
    }

    #[test]
    fn ecosystems_are_always_written_even_when_empty() {
        let fs = FakeFilesystem::new();
        let root = Path::new("/kb");
        let intents = catalog(root, &fs);
        let git = FakeGitClient::new();
        let clock = FakeClock::at(Utc.with_ymd_and_hms(2026, 9, 5, 14, 2, 11).unwrap());
        let paths = paths();
        let ctx = AppContext {
            fs: &fs,
            git: &git,
            clock: &clock,
            paths: &paths,
            llm: None,
        };
        let mut unfiltered = profile();
        unfiltered.select.ecosystems.clear();
        let manifest = build(root, &unfiltered, &assembly(&["a/first"]), &intents, &ctx).unwrap();
        assert!(manifest.to_json().unwrap().contains("\"ecosystems\": []"));
    }

    #[test]
    fn write_creates_parents_and_leaves_no_temp_file() {
        let fs = FakeFilesystem::new();
        let manifest = build_with(Path::new("/kb"), &fs, &["a/first"]);
        let path = Path::new("/project/.context-mixer/cmf-manifest.json");
        write(&manifest, path, &fs).unwrap();
        assert_eq!(
            fs.read_to_string(path).unwrap(),
            manifest.to_json().unwrap(),
            "file holds the rendered JSON"
        );
        assert!(!fs.exists(&json_file::tmp_path(path)), "temp file is renamed away");
    }

    #[test]
    fn local_manifest_path_sits_beside_the_local_lock_file() {
        let paths = paths();
        let manifest = local_manifest_path(&paths);
        assert_eq!(manifest, PathBuf::from(".context-mixer/cmf-manifest.json"));
        assert_eq!(manifest.parent(), paths.lock_path(InstallScope::Local).parent());
    }
}
