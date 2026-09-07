//! `cmf` binary entry point.

use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cmf::assembly::assemble;
use cmf::catalog;
use cmf::cli::{Cli, Commands, SurfaceArg};
use cmf::manifest;
use cmf::profile::{self, Surface};
use cmx_core::artifact_install::{ArtifactIdentity, ArtifactInstaller, BundledArtifact};
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
            let intents = catalog::scan(&root, &fs)?;
            let assembly = assemble(&profile, &intents)?;
            if explain {
                print_explanation(&profile_path, &profile, &assembly);
            }
            if let Some(manifest_path) = manifest_path {
                let production = ProductionContext::claude()?;
                let ctx = production.ctx();
                let manifest = manifest::build(&root, &profile, &assembly, &intents, &ctx)?;
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
            let intents = catalog::scan(&root, &fs)?;
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
            print!("{plan}");
            if apply {
                let report = installer.apply(&bundle, &plan, &ctx)?;
                print!("{report}");
                if let Some(manifest_path) = manifest_path {
                    let manifest = manifest::build(&root, &profile, &assembly, &intents, &ctx)?;
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
            let intents = catalog::scan(&root, &fs)?;
            let profiles = catalog::profile_count(&root, &fs)?;
            println!("Knowledge base: {}", root.display());
            println!("Structured intents: {}", intents.len());
            println!("Materialization profiles: {profiles}");
            if profiles == 0 {
                println!(
                    "No profiles found; add TOML profiles under profiles/ to assemble guidance."
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
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
