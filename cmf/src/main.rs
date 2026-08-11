//! `cmf` binary entry point.

use std::env;
use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use cmf::assembly::assemble;
use cmf::catalog;
use cmf::cli::{Cli, Commands, SurfaceArg};
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
        } => {
            let (mut profile, profile_path) = profile::load(&root, &profile, &fs)?;
            apply_surface(&mut profile.surface, surface);
            let intents = catalog::scan(&root, &fs)?;
            let assembly = assemble(&profile, &intents)?;
            if explain {
                print_explanation(&profile_path, &assembly);
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
                Surface::Agent => BundledArtifact::agent(assembly.content),
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
            eprintln!("Profile: {}", profile_path.display());
            print!("{plan}");
            if apply {
                let report = installer.apply(&bundle, &plan, &ctx)?;
                print!("{report}");
            } else {
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

fn print_explanation(profile_path: &std::path::Path, assembly: &cmf::assembly::Assembly) {
    eprintln!("profile: {}", profile_path.display());
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
