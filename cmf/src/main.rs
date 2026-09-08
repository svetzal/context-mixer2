//! `cmf` binary entry point: the imperative shell that reads the atlas
//! (catalog, then its sensors validated against that catalog), assembles,
//! previews, and applies installs through cmx-core, and — at `install
//! --local` — senses the current project's ecosystems and prints the
//! mismatch warning [`cmf::mismatch`] decides on.

use std::collections::BTreeMap;
use std::env;
use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cmf::assembly::assemble;
use cmf::catalog::{self, Intent};
use cmf::cli::{Cli, Commands, SurfaceArg};
use cmf::manifest;
use cmf::mismatch;
use cmf::profile::{self, Surface};
use cmf::sensors::{self, Detection, Sensors};
use cmx_core::artifact_install::{ArtifactIdentity, ArtifactInstaller, BundledArtifact};
use cmx_core::gateway::Filesystem;
use cmx_core::gateway::real::RealFilesystem;
use cmx_core::production::ProductionContext;
use cmx_core::skill_install::Scope;

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let root = cli.root.unwrap_or(env::current_dir()?);
    let fs = RealFilesystem;
    match cli.command {
        Commands::Assemble {
            profile,
            surface,
            explain,
            manifest: manifest_path,
        } => {
            let (mut profile, profile_path) = profile::load(&root, &profile, &fs)?;
            apply_surface(&mut profile.surface, surface);
            let (intents, _sensors) = scan_atlas(&root, &fs)?;
            let assembly = assemble(&profile, &intents)?;
            if explain {
                print_explanation(&profile_path, &profile, &assembly);
            }
            if let Some(manifest_path) = manifest_path {
                let production = ProductionContext::claude()?;
                let ctx = production.ctx();
                let manifest = manifest::build(
                    &root,
                    &profile,
                    &assembly.selected,
                    &assembly.content,
                    &intents,
                    &ctx,
                )?;
                manifest::write(&manifest, &manifest_path, ctx.fs)?;
                eprintln!("manifest: {}", manifest_path.display());
            }
            print!("{}", assembly.content);
        }
        Commands::Install {
            profile,
            surface,
            local,
            apply,
            force,
        } => {
            let (mut profile, profile_path) = profile::load(&root, &profile, &fs)?;
            apply_surface(&mut profile.surface, surface);
            let (intents, sensors) = scan_atlas(&root, &fs)?;
            let assembly = assemble(&profile, &intents)?;
            let bundle = match profile.surface {
                Surface::Agent => BundledArtifact::agent(assembly.content.clone()),
                Surface::Skill => BundledArtifact::skill_md(&assembly.content),
            };
            let installer = ArtifactInstaller::new(ArtifactIdentity::new(
                profile.artifact_name(),
                &profile.version,
            ));
            let production = ProductionContext::claude()?;
            let ctx = production.ctx();
            let scope = if local { Scope::Local } else { Scope::Global };
            let plan = installer.plan(&bundle, scope, force, &ctx)?;
            // Only a project-local install has one repository to verify, so only
            // it records a manifest (see CMV.md, "Open decisions").
            let manifest_path = local.then(|| manifest::local_manifest_path(ctx.paths));
            eprintln!("Profile: {}", profile_path.display());
            if local {
                warn_on_ecosystem_mismatch(&profile, sensors.as_ref(), &fs)?;
            }
            print!("{plan}");
            if apply {
                let report = installer.apply(&bundle, &plan, &ctx)?;
                print!("{report}");
                if let Some(manifest_path) = manifest_path {
                    let manifest = manifest::build(
                        &root,
                        &profile,
                        &assembly.selected,
                        &assembly.content,
                        &intents,
                        &ctx,
                    )?;
                    manifest::write(&manifest, &manifest_path, ctx.fs)?;
                    println!("Manifest written to {}", manifest_path.display());
                }
            } else {
                if let Some(manifest_path) = manifest_path {
                    println!("Manifest will be written to {}", manifest_path.display());
                }
                println!("Re-run with --apply to make these changes.");
            }
        }
        Commands::Status => {
            let (intents, sensors) = scan_atlas(&root, &fs)?;
            let profiles = catalog::profile_count(&root, &fs)?;
            println!("Atlas: {}", root.display());
            println!("Structured intents: {}", intents.len());
            println!("Materialization profiles: {profiles}");
            println!("Sensors: {}", describe_sensors(sensors.as_ref()));
            if profiles == 0 {
                println!(
                    "No profiles found; add TOML profiles under profiles/ to assemble guidance."
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Read the atlas: its catalog, then its sensors validated against that
/// catalog, so a sensor file that disagrees with the record hierarchy is an
/// atlas error at scan time.
fn scan_atlas(
    root: &Path,
    fs: &dyn Filesystem,
) -> Result<(BTreeMap<String, Intent>, Option<Sensors>)> {
    let intents = catalog::scan(root, fs)?;
    let sensors = sensors::load_validated(root, &intents, fs)?;
    Ok((intents, sensors))
}

/// Only a project-local install has one project to compare the profile's
/// ecosystems against: the current directory. Prints the warning
/// [`mismatch::warning`] decides on, if any.
fn warn_on_ecosystem_mismatch(
    profile: &profile::Profile,
    sensors: Option<&Sensors>,
    fs: &dyn Filesystem,
) -> Result<()> {
    let project = env::current_dir()?;
    let detection = Detection::sense(sensors, &project, fs);
    if let Some(warning) = mismatch::warning(profile, &detection) {
        eprintln!("warning: {warning}");
    }
    Ok(())
}

/// The `Sensors:` line body of `cmf status`.
fn describe_sensors(sensors: Option<&Sensors>) -> String {
    match sensors.map_or(0, Sensors::len) {
        0 => "none declared".to_string(),
        1 => "1 ecosystem".to_string(),
        count => format!("{count} ecosystems"),
    }
}

fn apply_surface(surface: &mut Surface, override_surface: Option<SurfaceArg>) {
    if let Some(override_surface) = override_surface {
        *surface = match override_surface {
            SurfaceArg::Agent => Surface::Agent,
            SurfaceArg::Skill => Surface::Skill,
        };
    }
}

fn print_explanation(
    profile_path: &std::path::Path,
    profile: &profile::Profile,
    assembly: &cmf::assembly::Assembly,
) {
    eprintln!("profile: {}", profile_path.display());
    let ecosystems = &profile.select.ecosystems;
    if ecosystems.is_empty() {
        eprintln!("ecosystems: (none declared; no filter applied)");
    } else {
        eprintln!("ecosystems: {}", ecosystems.join(", "));
        eprintln!(
            "excluded by ecosystem from category/tag matching: {}",
            assembly.excluded_by_ecosystem
        );
        if profile.graph.prefer_specializations {
            eprintln!("specialized downward: {}", assembly.specialized_downward);
        }
    }
    eprintln!("selected intents ({}):", assembly.selected.len());
    for key in &assembly.selected {
        eprintln!("  {key}");
    }
    if !assembly.traversed.is_empty() {
        eprintln!("graph traversal:");
        for edge in &assembly.traversed {
            eprintln!("  {edge}");
        }
    }
    eprintln!("estimated tokens: {}", assembly.estimated_tokens);
}
