mod app;
mod cli;
mod config;
mod gitignore;
mod self_updater;
mod template;
mod ui;
mod validation;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tokio::runtime::Runtime;

use app::App;
use cli::{Cli, Commands};
use ui::print_success;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cache_dir = cli
        .cache_dir
        .unwrap_or_else(|| dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".lightignore")));

    let app = App::new(cache_dir)?;
    let rt = Runtime::new()?;

    match cli.command.unwrap_or(Commands::Generate { output: None }) {
        Commands::Update => {
            rt.block_on(app.update_cache())?;
            print_success("Cache updated")?;
        }
        Commands::List => {
            let index = app.read_index()?;
            app.list_templates(&index)?;
        }
        Commands::Generate { output } => {
            let index = app.read_index()?;
            let output_path = output.unwrap_or_else(|| PathBuf::from(".gitignore"));
            app.generate_interactive(&index, output_path)?;
        }
        Commands::SelfUpdate => {
            self_updater::update()?;
        }
    }

    Ok(())
}
