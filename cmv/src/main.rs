//! `cmv` binary entry point: the imperative shell. Resolves the project, its
//! manifest, and the atlas from the command line and the cmx source
//! registry; loads them through the real gateways; materializes the pinned
//! revision when the checkout has moved past it; hands everything to the pure
//! core; prints the report; and exits per `CMV.md`'s table (`2` for anything
//! that stops the run before a verdict is reached).

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use cmv::cli::{Cli, Commands, LocationArgs};
use cmv::config::{self, ProjectConfig};
use cmv::dispatch::{self, Catalog, CheckRequest, StatusRequest};
use cmv::ecosystems::Ecosystems;
use cmv::explain::{self, ExplainRequest};
use cmv::pin::{self, AtlasReport, Checkout, PinPolicy};
use cmv::process::RealProcessRunner;
use cmv::report::{CheckReport, OutputFormat};
use cmv::resolve::{self, Resolution};
use cmv::verdict::{EXIT_USAGE, Strictness};
use cmx_core::gateway::Filesystem;
use cmx_core::gateway::real::RealFilesystem;
use cmx_core::paths::ConfigPaths;
use cmx_core::platform::Platform;
use intent_atlas::catalog;
use intent_atlas::manifest::{LOCAL_MANIFEST_FILE_NAME, Manifest, ManifestCollection};
use intent_atlas::sensors::{self, Sensors};
use serde::Serialize;

#[derive(Serialize)]
struct ArtifactCheckReport {
    artifact: intent_atlas::manifest::ArtifactRef,
    check: CheckReport,
}

#[derive(Serialize)]
struct MultiCheckReport {
    schema: u32,
    artifacts: Vec<ArtifactCheckReport>,
    summary: MultiSummary,
}

#[derive(Serialize)]
struct MultiSummary {
    exit_code: u8,
}

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
        } => run_check(location, OutputFormat::from_flag(json), Strictness::from_flag(strict), &fs),
        Commands::Status { json, location } => {
            let format = OutputFormat::from_flag(json);
            let site = Site::resolve(location, &fs)?;
            let scratch = Scratch::create(&fs)?;
            let checkout = site.checkout(&scratch, &fs)?;
            // `status` reports rather than fails when the atlas is absent or
            // cannot be read.
            let (catalog, sensors) = if fs.is_dir(&checkout.root) {
                match site.scan_atlas(&checkout, &fs) {
                    Ok((catalog, sensors)) => (Some(catalog), sensors),
                    Err(error) => {
                        eprintln!("warning: {error:#}");
                        (None, None)
                    }
                }
            } else {
                (None, None)
            };
            let ecosystems = site.ecosystems(sensors.as_ref(), &fs);
            let atlas = AtlasReport::new(&site.resolution, &checkout, sensors.as_ref());
            let request = StatusRequest {
                manifest: &site.manifest,
                manifest_path: &site.manifest_path,
                catalog: catalog.as_ref(),
                ecosystems: &ecosystems,
                atlas: &atlas,
                trees: checkout.trees(),
            };
            let report = dispatch::status(&request, &fs)?;
            print!("{}", report.render(format)?);
            Ok(ExitCode::SUCCESS)
        }
        Commands::Explain {
            intent,
            json,
            location,
        } => {
            let format = OutputFormat::from_flag(json);
            let site = Site::resolve(location, &fs)?;
            let scratch = Scratch::create(&fs)?;
            let checkout = site.checkout(&scratch, &fs)?;
            let (catalog, sensors) = site.scan_atlas(&checkout, &fs)?;
            let ecosystems = site.ecosystems(sensors.as_ref(), &fs);
            let atlas = AtlasReport::new(&site.resolution, &checkout, sensors.as_ref());
            let request = ExplainRequest {
                manifest: &site.manifest,
                catalog: &catalog,
                config: &site.config,
                ecosystems: &ecosystems,
                atlas: &atlas,
                trees: checkout.trees(),
                workspace: &site.root,
            };
            let report = explain::explain(&intent, &request, &fs)?;
            print!("{}", report.render(format)?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn run_check(
    location: LocationArgs,
    format: OutputFormat,
    strictness: Strictness,
    fs: &dyn Filesystem,
) -> Result<ExitCode> {
    let sites = Site::resolve_all(location, fs)?;
    let mut reports = Vec::new();
    for site in &sites {
        let scratch = Scratch::create(fs)?;
        let checkout = site.checkout(&scratch, fs)?;
        let (catalog, sensors) = site.scan_atlas(&checkout, fs)?;
        let ecosystems = site.ecosystems(sensors.as_ref(), fs);
        let request = CheckRequest {
            manifest: &site.manifest,
            catalog: &catalog,
            config: &site.config,
            ecosystems: &ecosystems,
            trees: checkout.trees(),
            workspace: &site.root,
            scratch: &scratch.dir,
        };
        let outcomes = dispatch::check(&request, fs, &RealProcessRunner)?;
        let atlas = AtlasReport::new(&site.resolution, &checkout, sensors.as_ref());
        let report = CheckReport::new(&site.manifest, atlas, ecosystems, outcomes, strictness);
        reports.push(ArtifactCheckReport {
            artifact: site.manifest.artifact.clone(),
            check: report,
        });
    }
    if reports.len() == 1 && sites[0].legacy_manifest {
        let report = &reports[0].check;
        print!("{}", report.render(format)?);
        return Ok(ExitCode::from(report.summary.exit_code));
    }
    let exit_code = reports.iter().map(|entry| entry.check.summary.exit_code).max().unwrap_or(0);
    match format {
        OutputFormat::Json => {
            let report = MultiCheckReport {
                schema: 2,
                artifacts: reports,
                summary: MultiSummary { exit_code },
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Human => {
            for entry in &reports {
                println!("{} ({:?})", entry.artifact.name, entry.artifact.surface);
                print!("{}", entry.check.render(format)?);
            }
        }
    }
    Ok(ExitCode::from(exit_code))
}

/// The project as resolved from the command line: root, manifest, config,
/// atlas, and whether to honour the pin.
struct Site {
    root: PathBuf,
    manifest_path: PathBuf,
    manifest: Manifest,
    config: ProjectConfig,
    resolution: Resolution,
    pin_policy: PinPolicy,
    legacy_manifest: bool,
}

impl Site {
    fn resolve(location: LocationArgs, fs: &dyn Filesystem) -> Result<Self> {
        Ok(Self::resolve_all(location, fs)?.remove(0))
    }

    fn resolve_all(location: LocationArgs, fs: &dyn Filesystem) -> Result<Vec<Self>> {
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
        let legacy_manifest = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|value| value.get("schema").and_then(serde_json::Value::as_u64))
            == Some(1);
        let collection = ManifestCollection::from_json(&raw)
            .with_context(|| format!("malformed manifest {}", manifest_path.display()))?;
        let config = config::load(&root, fs)?;
        // The platform only affects install-directory resolution, which cmv
        // never does; the sources registry is platform-independent.
        let paths = ConfigPaths::from_env(Platform::Claude)?;
        // A relative path — from the command line, the registry, or the
        // manifest — is interpreted against cmv's working directory, exactly
        // as it would be on the command line.
        collection
            .artifacts
            .into_iter()
            .map(|manifest| {
                let resolution =
                    resolve::resolve(location.atlas.as_deref(), &manifest.atlas, fs, &paths)?;
                if let Some(warning) = &resolution.warning {
                    eprintln!("warning: {warning}");
                }
                Ok(Self {
                    root: root.clone(),
                    manifest_path: manifest_path.clone(),
                    manifest,
                    config: config.clone(),
                    resolution,
                    pin_policy: PinPolicy::from_flag(location.at_head),
                    legacy_manifest,
                })
            })
            .collect()
    }

    /// Which tree to verify against, materializing the pinned revision under
    /// `scratch` when the checkout has moved past it.
    fn checkout(&self, scratch: &Scratch<'_>, fs: &dyn Filesystem) -> Result<Checkout> {
        pin::materialize(
            &self.resolution,
            self.manifest.atlas.revision.as_deref(),
            self.pin_policy,
            &scratch.dir,
            fs,
            &RealProcessRunner,
        )
    }

    /// Read the verified tree: its catalog, then its sensors validated
    /// against that catalog (the sensors are pinned with the records).
    fn scan_atlas(
        &self,
        checkout: &Checkout,
        fs: &dyn Filesystem,
    ) -> Result<(Catalog, Option<Sensors>)> {
        let catalog = catalog::scan(&checkout.root, fs).with_context(|| {
            format!(
                "could not read atlas at {}; pass --atlas <path> if it lives elsewhere",
                self.resolution.path.display()
            )
        })?;
        let sensors = sensors::load_validated(&checkout.root, &catalog, fs)?;
        Ok((catalog, sensors))
    }

    /// The ecosystems to verify as: the `cmv.toml` override, else what the
    /// atlas's sensors detect at the project root.
    fn ecosystems(&self, sensors: Option<&Sensors>, fs: &dyn Filesystem) -> Ecosystems {
        Ecosystems::resolve(self.config.ecosystems.as_deref(), sensors, &self.root, fs)
    }
}

/// Where `cmf install --local` writes the manifest: `.context-mixer/` under the
/// project root, beside the local lock file (`intent_atlas::manifest::local_manifest_path`
/// gives the same relative location from cmx-core's `ConfigPaths`).
fn default_manifest_path(root: &Path) -> PathBuf {
    root.join(".context-mixer").join(LOCAL_MANIFEST_FILE_NAME)
}

/// A per-run temporary directory for the validators' `--config` files and a
/// materialized pinned tree, removed when dropped.
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
