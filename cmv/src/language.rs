//! Language detection from the project's root-level build manifests: the set
//! of languages cmv verifies as, and therefore which validators run. Pure over
//! the `Filesystem` gateway; a `languages` override in `cmv.toml` replaces
//! detection entirely.

use std::path::Path;

use cmx_core::gateway::Filesystem;

/// Detect the workspace's languages from files at `root` (not below it).
///
/// - `Cargo.toml` → `rust`
/// - `pyproject.toml`, `setup.cfg`, `setup.py`, or any `requirements*.txt` →
///   `python`
/// - `package.json` → `typescript` when a `tsconfig.json` sits beside it,
///   else `javascript`
/// - `go.mod` → `go`
///
/// The result is sorted and deduplicated so it reads the same in every
/// report.
pub fn detect(root: &Path, fs: &dyn Filesystem) -> Vec<String> {
    let present = |name: &str| fs.is_file(&root.join(name));
    let mut languages = Vec::new();
    if present("Cargo.toml") {
        languages.push("rust");
    }
    if present("pyproject.toml")
        || present("setup.cfg")
        || present("setup.py")
        || has_requirements_file(root, fs)
    {
        languages.push("python");
    }
    if present("package.json") {
        languages.push(if present("tsconfig.json") {
            "typescript"
        } else {
            "javascript"
        });
    }
    if present("go.mod") {
        languages.push("go");
    }
    normalize(languages.into_iter().map(str::to_string).collect())
}

/// The languages to verify as: the `cmv.toml` override when present, else what
/// [`detect`] found. Either way the list is sorted and deduplicated.
pub fn resolve(override_languages: Option<&[String]>, detected: Vec<String>) -> Vec<String> {
    match override_languages {
        Some(languages) => normalize(languages.to_vec()),
        None => detected,
    }
}

fn has_requirements_file(root: &Path, fs: &dyn Filesystem) -> bool {
    fs.read_dir(root).is_ok_and(|entries| {
        entries.iter().any(|entry| {
            !entry.is_dir
                && entry.file_name.starts_with("requirements")
                && Path::new(&entry.file_name)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
        })
    })
}

fn normalize(mut languages: Vec<String>) -> Vec<String> {
    languages.sort();
    languages.dedup();
    languages
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmx_core::gateway::fakes::FakeFilesystem;

    const ROOT: &str = "/project";

    fn detect_with(files: &[&str]) -> Vec<String> {
        let fs = FakeFilesystem::new();
        fs.add_dir(ROOT);
        for file in files {
            fs.add_file(format!("{ROOT}/{file}"), "");
        }
        detect(Path::new(ROOT), &fs)
    }

    #[test]
    fn nothing_detected_in_an_empty_or_missing_root() {
        assert!(detect_with(&[]).is_empty());
        let fs = FakeFilesystem::new();
        assert!(detect(Path::new("/nowhere"), &fs).is_empty());
    }

    #[test]
    fn cargo_manifest_means_rust() {
        assert_eq!(detect_with(&["Cargo.toml"]), ["rust"]);
    }

    #[test]
    fn any_python_packaging_file_means_python_once() {
        assert_eq!(detect_with(&["pyproject.toml"]), ["python"]);
        assert_eq!(detect_with(&["setup.cfg"]), ["python"]);
        assert_eq!(detect_with(&["setup.py"]), ["python"]);
        assert_eq!(detect_with(&["requirements.txt"]), ["python"]);
        assert_eq!(detect_with(&["requirements-dev.txt"]), ["python"]);
        assert_eq!(detect_with(&["pyproject.toml", "setup.py", "requirements.txt"]), ["python"]);
    }

    #[test]
    fn requirements_must_be_a_txt_file_at_the_root() {
        assert!(detect_with(&["requirements.in"]).is_empty());
        assert!(detect_with(&["docs/requirements.txt"]).is_empty());
    }

    #[test]
    fn package_json_is_typescript_only_with_tsconfig() {
        assert_eq!(detect_with(&["package.json"]), ["javascript"]);
        assert_eq!(detect_with(&["package.json", "tsconfig.json"]), ["typescript"]);
        assert!(detect_with(&["tsconfig.json"]).is_empty(), "tsconfig alone is not a package");
    }

    #[test]
    fn go_module_means_go() {
        assert_eq!(detect_with(&["go.mod"]), ["go"]);
    }

    #[test]
    fn several_languages_are_sorted() {
        assert_eq!(
            detect_with(&["go.mod", "Cargo.toml", "pyproject.toml", "package.json"]),
            ["go", "javascript", "python", "rust"]
        );
    }

    #[test]
    fn nested_manifests_do_not_count() {
        assert!(detect_with(&["crates/core/Cargo.toml", "tools/go.mod"]).is_empty());
    }

    #[test]
    fn override_replaces_detection_entirely() {
        let detected = vec!["rust".to_string()];
        let languages = vec!["python".to_string(), "go".to_string(), "python".to_string()];
        assert_eq!(resolve(Some(&languages), detected.clone()), ["go", "python"]);
        assert_eq!(
            resolve(Some(&[]), detected.clone()),
            Vec::<String>::new(),
            "an empty override verifies nothing"
        );
        assert_eq!(resolve(None, detected), ["rust"]);
    }
}
