//! Read-only atlas validation using the same scan and assembly rules as install.

use std::fmt;
use std::path::{Path, PathBuf};

use cmx_core::gateway::Filesystem;
use intent_atlas::{catalog, profile, sensors};
use serde::Serialize;

use crate::assembly::assemble;

/// One source-located validation failure.
#[derive(Debug, Serialize)]
pub struct Diagnostic {
    /// Source file or atlas directory involved.
    pub file: PathBuf,
    /// Schema field or operation that failed.
    pub field: String,
    /// Human-readable cause.
    pub message: String,
}

/// Deterministic validation result. Invalid atlases exit with status 2.
#[derive(Debug, Serialize)]
pub struct Report {
    /// Whether all checks passed.
    pub valid: bool,
    /// Successfully scanned intent count.
    pub intents: usize,
    /// Successfully assembled profile count.
    pub profiles: usize,
    /// Failures in traversal order.
    pub diagnostics: Vec<Diagnostic>,
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.valid {
            return writeln!(
                f,
                "Atlas valid: {} intents, {} profiles",
                self.intents, self.profiles
            );
        }
        for diagnostic in &self.diagnostics {
            writeln!(
                f,
                "{} [{}]: {}",
                diagnostic.file.display(),
                diagnostic.field,
                diagnostic.message
            )?;
        }
        Ok(())
    }
}

/// Inspect records, sensors, profiles, selection, budgets, and validator paths.
/// No validator is invoked and no atlas or project file is written.
pub fn atlas(root: &Path, fs: &dyn Filesystem) -> Report {
    let mut report = Report {
        valid: false,
        intents: 0,
        profiles: 0,
        diagnostics: Vec::new(),
    };
    let (intents, record_errors) = match catalog::scan_all(root, fs) {
        Ok(result) => result,
        Err(error) => {
            report
                .diagnostics
                .push(record_error(&root.join("intents"), &format!("{error:#}")));
            return report;
        }
    };
    for (path, error) in record_errors {
        report.diagnostics.push(record_error(&path, &format!("{error:#}")));
    }
    report.intents = intents.len();
    if let Err(error) = sensors::load_validated(root, &intents, fs) {
        report.diagnostics.push(Diagnostic {
            file: root.join(sensors::SENSOR_FILE_NAME),
            field: "ecosystems".into(),
            message: format!("{error:#}"),
        });
    }
    let root_canonical = std::fs::canonicalize(root);
    for intent in intents.values() {
        for (index, evidence) in intent.record.evidence.iter().enumerate() {
            let Some(validator) = evidence.validator() else {
                continue;
            };
            let path = root.join(validator.run);
            let valid = !validator.run.is_absolute()
                && root_canonical
                    .as_ref()
                    .ok()
                    .zip(std::fs::canonicalize(&path).ok())
                    .is_some_and(|(root, target)| target.starts_with(root) && executable(&target));
            if !valid {
                report.diagnostics.push(Diagnostic {
                    file: intent.path.clone(),
                    field: format!("evidence[{}].run", index + 1),
                    message: format!(
                        "validator {} must be an executable file inside the atlas",
                        path.display()
                    ),
                });
            }
        }
    }
    let profiles_dir = root.join("profiles");
    if fs.is_dir(&profiles_dir) {
        let mut paths = Vec::new();
        if let Err(error) = collect_profiles(&profiles_dir, fs, &mut paths) {
            report.diagnostics.push(Diagnostic {
                file: profiles_dir,
                field: "profiles".into(),
                message: format!("{error:#}"),
            });
        } else {
            paths.sort();
            for path in paths {
                match profile::load(root, &path, fs) {
                    Ok((profile, _)) => match assemble(&profile, &intents) {
                        Ok(_) => report.profiles += 1,
                        Err(error) => {
                            let message = format!("{error:#}");
                            let field = if message.contains("budget") {
                                "budget_tokens"
                            } else {
                                "select"
                            };
                            // A malformed record can make a profile appear
                            // unselectable; report both source failures.
                            report.diagnostics.push(Diagnostic {
                                file: path,
                                field: field.into(),
                                message,
                            });
                        }
                    },
                    Err(error) => {
                        let message = format!("{error:#}");
                        report.diagnostics.push(Diagnostic {
                            file: path,
                            field: profile_field(&message),
                            message,
                        });
                    }
                }
            }
        }
    }
    report.valid = report.diagnostics.is_empty();
    report
}

fn profile_field(message: &str) -> String {
    if let Some(field) =
        message.split("missing field `").nth(1).and_then(|rest| rest.split('`').next())
    {
        return field.into();
    }
    if message.contains("budget_tokens") {
        "budget_tokens"
    } else if message.contains("graph relationship") {
        "graph.follow"
    } else if message.contains("content section") {
        "content.include"
    } else if message.contains("must select") {
        "select"
    } else {
        "profile"
    }
    .into()
}

fn record_error(path: &Path, message: &str) -> Diagnostic {
    let mut field = "intents".to_string();
    if let Some(rest) = message.strip_prefix("intent ") {
        if let Some((_, entry)) = rest.split_once(" evidence entry ") {
            let number = entry.split_whitespace().next().unwrap_or("?");
            let member = if message.contains("`run` is missing")
                || message.contains("validator path")
                || message.contains("`run` may only")
            {
                "run"
            } else if message.contains("`language` is missing")
                || message.contains("`language` may only")
            {
                "language"
            } else {
                "evidence"
            };
            field = format!("evidence[{number}].{member}");
        }
    } else if message.starts_with("could not parse intent ") {
        field = message
            .split("missing field `")
            .nth(1)
            .and_then(|rest| rest.split('`').next())
            .unwrap_or("record")
            .into();
    }
    Diagnostic {
        file: path.to_path_buf(),
        field,
        message: message.to_string(),
    }
}

fn collect_profiles(
    dir: &Path,
    fs: &dyn Filesystem,
    paths: &mut Vec<PathBuf>,
) -> anyhow::Result<()> {
    for entry in fs.read_dir(dir)? {
        if entry.is_dir {
            collect_profiles(&entry.path, fs, paths)?;
        } else if entry.path.extension().is_some_and(|ext| ext == "toml") {
            paths.push(entry.path);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    path.is_file()
}
