use anyhow::{Result, bail};
use clap::Parser;

use cmx::cli::{ArtifactAction, Cli, Commands, ConfigAction, SourceAction};
use cmx::context::AppContext;
use cmx::gateway::real::{RealFilesystem, RealGitClient, SystemClock};
use cmx::paths::ConfigPaths;
use cmx::types::ArtifactKind;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = ConfigPaths::from_env()?;

    let ctx = AppContext {
        fs: &RealFilesystem,
        git: &RealGitClient,
        clock: &SystemClock,
        paths: &paths,
        llm: None,
    };

    match cli.command {
        Commands::Source { action } => handle_source(action, &paths, &ctx),
        Commands::Agent { action } => handle_artifact(action, ArtifactKind::Agent, &ctx),
        Commands::Skill { action } => handle_artifact(action, ArtifactKind::Skill, &ctx),
        Commands::List => {
            let output = cmx::list::list_all_with(&ctx)?;
            cmx::display::print_list_all_output(&output);
            Ok(())
        }
        Commands::Info { name } => {
            let info = cmx::info::info_with(&name, &ctx)?;
            cmx::info::print_info(&info);
            Ok(())
        }
        Commands::Outdated => {
            let rows = cmx::outdated::outdated_with(&ctx)?;
            cmx::outdated::print_outdated(&rows);
            Ok(())
        }
        Commands::Search { query } => {
            let output = cmx::search::search_with(&query, &ctx)?;
            cmx::search::print_search_results(&output);
            Ok(())
        }
        Commands::Config { action } => handle_config(action, &ctx),
    }
}

fn handle_source(action: SourceAction, paths: &ConfigPaths, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        SourceAction::Add { name, path_or_url } => {
            if cmx::source::looks_like_url(&path_or_url) {
                let clone_dir = paths.git_clones_dir().join(&name);
                println!("Cloning {path_or_url} to {}...", clone_dir.display());
            }
            let result = cmx::source::add_with(&name, &path_or_url, ctx)?;
            println!(
                "Source '{}' registered: {} agent(s), {} skill(s) found.",
                result.name, result.agents_found, result.skills_found
            );
            for warning in &result.warnings {
                eprintln!("Warning: {}", warning.message);
            }
            Ok(())
        }
        SourceAction::List => {
            let result = cmx::source::list_with(ctx)?;
            cmx::display::print_source_list(&result);
            Ok(())
        }
        SourceAction::Browse { name } => {
            let result = cmx::source::browse_with(&name, ctx)?;
            cmx::display::print_browse_result(&result);
            Ok(())
        }
        SourceAction::Update { name } => {
            match cmx::source::update_with(name.as_deref(), ctx)? {
                cmx::source::SourceUpdateOutput::NoGitSources => {
                    println!("No git-backed sources to update.");
                }
                cmx::source::SourceUpdateOutput::SingleUpdate(result) => {
                    println!(
                        "Source '{}': {} agent(s), {} skill(s).",
                        result.name, result.agents_found, result.skills_found
                    );
                }
                cmx::source::SourceUpdateOutput::BatchUpdate(results) => {
                    for result in &results {
                        println!(
                            "Source '{}': {} agent(s), {} skill(s).",
                            result.name, result.agents_found, result.skills_found
                        );
                    }
                }
            }
            Ok(())
        }
        SourceAction::Remove { name } => {
            let result = cmx::source::remove_with(&name, ctx)?;
            if result.clone_deleted {
                println!("Source '{}' removed (cloned repo deleted).", result.name);
            } else {
                println!("Source '{}' removed.", result.name);
            }
            Ok(())
        }
    }
}

fn handle_config(action: ConfigAction, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ConfigAction::Show => {
            let result = cmx::cmx_config::show_with(ctx)?;
            println!("LLM gateway: {}", result.gateway);
            println!("LLM model:   {}", result.model);
            Ok(())
        }
        ConfigAction::Gateway { value } => {
            let result = cmx::cmx_config::set_gateway_with(&value, ctx)?;
            println!("LLM gateway set to: {}", result.value);
            Ok(())
        }
        ConfigAction::Model { value } => {
            let result = cmx::cmx_config::set_model_with(&value, ctx)?;
            println!("LLM model set to: {}", result.value);
            Ok(())
        }
    }
}

fn print_install_result(result: &cmx::install::InstallResult) {
    let version_info = result.version.as_deref().map(|v| format!(" v{v}")).unwrap_or_default();
    println!(
        "Installed {}{version_info} ({}) from '{}' -> {}",
        result.artifact_name,
        result.kind,
        result.source_name,
        result.dest_dir.display()
    );
}

fn handle_artifact(action: ArtifactAction, kind: ArtifactKind, ctx: &AppContext<'_>) -> Result<()> {
    match action {
        ArtifactAction::Install {
            name,
            all,
            local,
            force,
        } => {
            if all {
                let result = cmx::install::install_all_with(kind, local, force, ctx)?;
                if result.installed.is_empty() {
                    println!(
                        "All available {}s are already installed and up to date.",
                        result.kind
                    );
                } else {
                    for r in &result.installed {
                        print_install_result(r);
                    }
                }
                Ok(())
            } else if let Some(name) = name {
                let result = cmx::install::install_with(&name, kind, local, force, ctx)?;
                print_install_result(&result);
                Ok(())
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::List => {
            let output = cmx::list::list_kind_with(kind, ctx)?;
            cmx::display::print_list_kind_output(&output);
            Ok(())
        }
        #[cfg(feature = "llm")]
        ArtifactAction::Diff { name } => {
            // Diff needs the LLM client — construct it from config and drive
            // the async call with a single-threaded runtime built on demand.
            use cmx::gateway::real::MojenticLlmClient;
            let cfg = cmx::config::load_config_with(ctx.fs, ctx.paths)?;
            let llm = MojenticLlmClient::new(cfg.llm);
            let diff_ctx = AppContext {
                fs: ctx.fs,
                git: ctx.git,
                clock: ctx.clock,
                paths: ctx.paths,
                llm: Some(&llm),
            };
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
            let output = rt.block_on(cmx::diff::diff_with(&name, kind, &diff_ctx))?;
            cmx::diff::print_diff_output(&output);
            Ok(())
        }
        ArtifactAction::Update { name, all, force } => {
            if all {
                let result = cmx::install::update_all_with(kind, force, ctx)?;
                if result.updated.is_empty() {
                    println!("All tracked {}s are up to date.", result.kind);
                } else {
                    for r in &result.updated {
                        print_install_result(r);
                    }
                }
                Ok(())
            } else if let Some(name) = name {
                let result = cmx::install::update_with(&name, kind, force, ctx)?;
                print_install_result(&result);
                Ok(())
            } else {
                bail!("Provide an artifact name or use --all")
            }
        }
        ArtifactAction::Uninstall { name, local } => {
            let result = cmx::uninstall::uninstall_with(&name, kind, local, ctx)?;
            cmx::uninstall::print_uninstall_result(&result);
            Ok(())
        }
    }
}
