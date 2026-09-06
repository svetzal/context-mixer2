//! `cmv` binary entry point: the imperative shell. Resolves the project, its
//! manifest, and the knowledge base from the command line; loads them through
//! the real gateways; hands everything to the pure core; prints the report;
//! and exits per `CMV.md`'s table (`2` for anything that stops the run before
//! a verdict is reached).

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cmf::catalog;
use cmf::manifest::{LOCAL_MANIFEST_FILE_NAME, Manifest};
use cmv::cli::{Cli, Commands, LocationArgs};
use cmv::config::{self, ProjectConfig};
use cmv::dispatch::{self, Catalog, CheckRequest};
use cmv::language;
use cmv::process::RealProcessRunner;
use cmv::report::{CheckReport, OutputFormat};
use cmv::verdict::{EXIT_USAGE, Strictness};
use cmx_core::gateway::Filesystem;
use cmx_core::gateway::real::RealFilesystem;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let fs = RealFilesystem;
    match cli.command {
        Commands::Check {
            json,
            strict,
            location,
        } => {
            let format = OutputFormat::from_flag(json);
            let strictness = Strictness::from_flag(strict);
            let site = Site::resolve(location, &fs)?;
            let catalog = catalog::scan(&site.knowledge_base, &fs).with_context(|| {
                format!(
                    "could not read knowledge base at {}; pass --knowledge-base <path> if it lives elsewhere",
                    site.knowledge_base.display()
                )
            })?;
            let languages = site.languages(&fs);
            let scratch = Scratch::create(&fs)?;
            let request = CheckRequest {
                manifest: &site.manifest,
                catalog: &catalog,
                config: &site.config,
                languages: &languages,
                knowledge_base: &site.knowledge_base,
                workspace: &site.root,
                scratch: &scratch.dir,
            };
            let outcomes = dispatch::check(&request, &fs, &RealProcessRunner)?;
            drop(scratch);
            let report = CheckReport::new(&site.manifest, &languages, outcomes, strictness);
            print!("{}", report.render(format)?);
            Ok(ExitCode::from(report.summary.exit_code))
        }
        Commands::Status { json, location } => {
            let format = OutputFormat::from_flag(json);
            let site = Site::resolve(location, &fs)?;
            let languages = site.languages(&fs);
            let catalog = scan_if_present(&site.knowledge_base, &fs);
            let report = dispatch::status(
                &site.manifest,
                &site.manifest_path,
                catalog.as_ref(),
                &languages,
                &site.knowledge_base,
                &fs,
            )?;
            print!("{}", report.render(format)?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The project as resolved from the command line: root, manifest, config, and
/// knowledge base.
struct Site {
    root: PathBuf,
    manifest_path: PathBuf,
    manifest: Manifest,
    config: ProjectConfig,
    knowledge_base: PathBuf,
}

impl Site {
    fn resolve(location: LocationArgs, fs: &dyn Filesystem) -> Result<Self> {
        let root = match location.root {
            Some(root) => root,
            None => env::current_dir().context("could not determine the current directory")?,
        };
        let manifest_path = location.manifest.unwrap_or_else(|| default_manifest_path(&root));
        if !fs.is_file(&manifest_path) {
            bail!(
                "no compile manifest at {}; run `cmf install --local --apply` in this project to write one, or pass --manifest <path>",
                manifest_path.display()
            );
        }
        let raw = fs
            .read_to_string(&manifest_path)
            .with_context(|| format!("could not read manifest {}", manifest_path.display()))?;
        let manifest: Manifest = serde_json::from_str(&raw)
            .with_context(|| format!("malformed manifest {}", manifest_path.display()))?;
        let config = config::load(&root, fs)?;
        // The manifest records the root as cmf was given it; a relative path
        // there is interpreted against cmv's working directory, exactly as it
        // would be on the command line.
        let knowledge_base =
            location.knowledge_base.unwrap_or_else(|| manifest.knowledge_base.path.clone());
        Ok(Self {
            root,
            manifest_path,
            manifest,
            config,
            knowledge_base,
        })
    }

    fn languages(&self, fs: &dyn Filesystem) -> Vec<String> {
        language::resolve(self.config.languages.as_deref(), language::detect(&self.root, fs))
    }
}

/// Where `cmf install --local` writes the manifest: `.context-mixer/` under the
/// project root, beside the local lock file (`cmf::manifest::local_manifest_path`
/// gives the same relative location from cmx-core's `ConfigPaths`).
fn default_manifest_path(root: &Path) -> PathBuf {
    root.join(".context-mixer").join(LOCAL_MANIFEST_FILE_NAME)
}

/// Scan the knowledge base for `status`, which reports rather than fails when
/// it cannot be read.
fn scan_if_present(knowledge_base: &Path, fs: &dyn Filesystem) -> Option<Catalog> {
    if !fs.is_dir(knowledge_base) {
        return None;
    }
    match catalog::scan(knowledge_base, fs) {
        Ok(catalog) => Some(catalog),
        Err(error) => {
            eprintln!(
                "warning: could not read knowledge base at {}: {error:#}",
                knowledge_base.display()
            );
            None
        }
    }
}

/// A per-run temporary directory for the validators' `--config` files,
/// removed when dropped.
struct Scratch<'a> {
    dir: PathBuf,
    fs: &'a dyn Filesystem,
}

impl<'a> Scratch<'a> {
    fn create(fs: &'a dyn Filesystem) -> Result<Self> {
        let dir = env::temp_dir().join(format!("cmv-{}", std::process::id()));
        fs.create_dir_all(&dir)
            .with_context(|| format!("could not create scratch directory {}", dir.display()))?;
        Ok(Self { dir, fs })
    }
}

impl Drop for Scratch<'_> {
    fn drop(&mut self) {
        // Best effort: a leftover scratch directory is harmless, and a failure
        // here must not mask the verdict.
        let _ = self.fs.remove_dir_all(&self.dir);
    }
}
