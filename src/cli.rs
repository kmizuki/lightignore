use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "lightignore - Interactive gitignore generator"
)]
pub struct Cli {
    /// Cache directory for downloaded templates
    #[arg(short, long)]
    pub cache_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Update the local cache of gitignore templates
    Update,
    /// List available templates
    List,
    /// Interactively build a .gitignore
    Generate {
        /// Output file path (default: ./.gitignore)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Update lightignore to the latest version
    SelfUpdate,
}
