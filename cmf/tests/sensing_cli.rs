//! End-to-end tests of ecosystem sensing through the `cmf` binary: at
//! `install --local` the profile's declared ecosystems are compared with what
//! the fixture atlas's sensors detect in the current directory, and `status`
//! counts the declared sensors. Only the preview runs — never `--apply` — so
//! nothing is installed. `HOME` points at a temp dir so the developer's own
//! cmx configuration never takes part.

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn fixture_kb() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manifest-kb")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create target dir");
    for entry in fs::read_dir(from).expect("fixture dir readable") {
        let entry = entry.expect("fixture entry");
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy fixture file");
        }
    }
}

/// A temp project directory (the cwd cmf senses) beside an isolated `HOME`.
struct Project {
    tmp: TempDir,
    dir: PathBuf,
}

impl Project {
    fn with_root_files(files: &[&str]) -> Self {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = tmp.path().join("project");
        fs::create_dir_all(&dir).expect("project dir");
        fs::create_dir_all(tmp.path().join("home")).expect("home dir");
        for file in files {
            fs::write(dir.join(file), "").expect("root file written");
        }
        Self { tmp, dir }
    }

    fn cmf(&self, atlas: &Path, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_cmf"))
            .arg("--root")
            .arg(atlas)
            .args(args)
            .current_dir(&self.dir)
            .env("HOME", self.tmp.path().join("home"))
            .output()
            .expect("cmf runs")
    }

    /// Preview a local install of the fixture's rust profile; the warning, if
    /// any, is on stderr.
    fn preview(&self, atlas: &Path) -> (Output, String) {
        let output = self.cmf(atlas, &["install", "rust-shipping", "--local"]);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(output.status.code(), Some(0), "{stderr}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("Re-run with --apply"),
            "preview only"
        );
        (output, stderr)
    }
}

#[test]
fn local_install_preview_warns_when_the_project_is_another_ecosystem() {
    let project = Project::with_root_files(&["pyproject.toml"]);
    let (_, stderr) = project.preview(&fixture_kb());
    assert!(
        stderr.contains(
            "warning: profile rust-shipping targets rust but this project shows python\n"
        ),
        "{stderr}"
    );
    assert!(!project.dir.join(".context-mixer").exists(), "the preview writes nothing");
}

#[test]
fn local_install_preview_is_quiet_when_the_project_matches() {
    let project = Project::with_root_files(&["Cargo.toml"]);
    let (_, stderr) = project.preview(&fixture_kb());
    assert!(!stderr.contains("warning:"), "{stderr}");
}

#[test]
fn local_install_preview_says_when_nothing_was_detected_or_no_sensors_exist() {
    let project = Project::with_root_files(&["README.md"]);
    let (_, stderr) = project.preview(&fixture_kb());
    assert!(
        stderr.contains("warning: profile rust-shipping targets rust but nothing was detected\n"),
        "{stderr}"
    );

    let atlas = project.tmp.path().join("kb");
    copy_tree(&fixture_kb(), &atlas);
    fs::remove_file(atlas.join("ecosystems.toml")).expect("fixture sensor file exists");
    let (_, stderr) = project.preview(&atlas);
    assert!(
        stderr.contains(
            "warning: profile rust-shipping targets rust but the atlas declares no sensors\n"
        ),
        "{stderr}"
    );
}

#[test]
fn status_counts_the_declared_sensors() {
    let project = Project::with_root_files(&[]);
    let output = project.cmf(&fixture_kb(), &["status"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Sensors: 2 ecosystems\n"), "{text}");

    let atlas = project.tmp.path().join("kb");
    copy_tree(&fixture_kb(), &atlas);
    fs::remove_file(atlas.join("ecosystems.toml")).expect("fixture sensor file exists");
    let output = project.cmf(&atlas, &["status"]);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Sensors: none declared\n"), "{text}");
}

#[test]
fn an_invalid_sensor_file_is_an_atlas_error_at_scan_time() {
    let project = Project::with_root_files(&["Cargo.toml"]);
    let atlas = project.tmp.path().join("kb");
    copy_tree(&fixture_kb(), &atlas);
    fs::write(
        atlas.join("ecosystems.toml"),
        "[python]\nsignatures = [{ file = \"pyproject.toml\" }]\n[uv]\nsignatures = [{ file = \"uv.lock\" }]\n",
    )
    .unwrap();
    for args in [
        &["status"][..],
        &["assemble", "rust-shipping"][..],
        &["install", "rust-shipping", "--local"][..],
    ] {
        let output = project.cmf(&atlas, args);
        assert_ne!(output.status.code(), Some(0), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("invalid sensors"), "{args:?}: {stderr}");
        assert!(
            stderr.contains("sensor ecosystem `uv` is not a directory"),
            "{args:?}: {stderr}"
        );
    }
}
