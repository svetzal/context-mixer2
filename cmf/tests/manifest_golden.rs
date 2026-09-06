//! Golden test: the compile manifest, byte for byte.
//!
//! The fixture knowledge base under `tests/fixtures/manifest-kb/` is loaded
//! into the in-memory gateways at a fixed root, with a fake git checkout, a
//! fake clock, and a cmx sources registry naming the root — so the serialized
//! manifest is fully deterministic and pinned by `expected-manifest.json`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use cmf::assembly::assemble;
use cmf::catalog::{self, Intent};
use cmf::manifest;
use cmx_core::config;
use cmx_core::context::AppContext;
use cmx_core::gateway::fakes::{FakeClock, FakeFilesystem, FakeGitClient};
use cmx_core::paths::ConfigPaths;
use cmx_core::test_support::make_local_entry;
use cmx_core::types::SourcesFile;

const KB_ROOT: &str = "/kb";
const HEAD_COMMIT: &str = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manifest-kb")
}

/// Mirror every fixture file into `fs` below `KB_ROOT`, preserving bytes.
fn load_fixture_tree(dir: &Path, target: &Path, fs: &FakeFilesystem) {
    for entry in fs::read_dir(dir).expect("fixture directory exists") {
        let entry = entry.expect("readable fixture entry");
        let dest = target.join(entry.file_name());
        if entry.path().is_dir() {
            load_fixture_tree(&entry.path(), &dest, fs);
        } else {
            fs.add_file(dest, fs::read(entry.path()).expect("readable fixture file"));
        }
    }
}

struct Fixture {
    fs: FakeFilesystem,
    paths: ConfigPaths,
}

fn fixture() -> Fixture {
    let fs = FakeFilesystem::new();
    let root = Path::new(KB_ROOT);
    for sub in ["intents", "profiles"] {
        load_fixture_tree(&fixture_dir().join(sub), &root.join(sub), &fs);
    }
    fs.add_file(root.join(".git/HEAD"), "ref: refs/heads/main\n");
    fs.add_file(root.join(".git/refs/heads/main"), format!("{HEAD_COMMIT}\n"));

    let paths =
        ConfigPaths::for_test("/home/tester".into(), "/home/tester/.config/context-mixer".into());
    let mut sources = SourcesFile::default();
    sources
        .sources
        .insert("guidelines".to_string(), make_local_entry(KB_ROOT, None));
    config::save_sources(&sources, &fs, &paths).expect("sources registry saved");
    Fixture { fs, paths }
}

fn build_manifest(fixture: &Fixture) -> (manifest::Manifest, BTreeMap<String, Intent>) {
    let root = Path::new(KB_ROOT);
    let (profile, _) =
        cmf::profile::load(root, Path::new("rust-shipping"), &fixture.fs).expect("profile loads");
    let intents = catalog::scan(root, &fixture.fs).expect("catalog scans");
    let assembly = assemble(&profile, &intents).expect("profile assembles");
    let git = FakeGitClient::new();
    let clock = FakeClock::at(Utc.with_ymd_and_hms(2026, 9, 5, 14, 2, 11).unwrap());
    let ctx = AppContext {
        fs: &fixture.fs,
        git: &git,
        clock: &clock,
        paths: &fixture.paths,
        llm: None,
    };
    let manifest =
        manifest::build(root, &profile, &assembly, &intents, &ctx).expect("manifest builds");
    (manifest, intents)
}

#[test]
fn manifest_matches_golden_fixture_byte_for_byte() {
    let fixture = fixture();
    let (manifest, _) = build_manifest(&fixture);
    let actual = manifest.to_json().expect("manifest serializes");
    let expected_path = fixture_dir().join("expected-manifest.json");
    let expected = fs::read_to_string(&expected_path).expect("golden file exists");
    assert_eq!(
        actual,
        expected,
        "manifest drifted from {}; if the change is intended, update the golden file",
        expected_path.display()
    );
}

#[test]
fn golden_intent_checksums_are_the_fixture_files_sha256() {
    let fixture = fixture();
    let (manifest, intents) = build_manifest(&fixture);
    assert_eq!(manifest.intents.len(), 2, "two of the three fixture records are selected");
    for entry in &manifest.intents {
        let on_disk = fs::read(fixture_dir().join("intents").join(format!("{}.toml", entry.key)))
            .expect("fixture record readable");
        assert_eq!(
            entry.checksum,
            cmx_core::checksum::checksum_bytes(&on_disk),
            "{} checksum is cmx-core's sha256 of the record bytes",
            entry.key
        );
        assert_eq!(entry.id, intents[&entry.key].record.id);
    }
}

#[test]
fn golden_manifest_writes_through_the_filesystem_gateway() {
    let fixture = fixture();
    let (manifest, _) = build_manifest(&fixture);
    let path = manifest::local_manifest_path(&fixture.paths);
    manifest::write(&manifest, &path, &fixture.fs).expect("manifest writes");
    let written = fixture.fs.get_file_content(&path).expect("manifest file exists");
    assert_eq!(String::from_utf8(written).unwrap(), manifest.to_json().unwrap());
}
