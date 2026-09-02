mod forge;
mod git;
mod manifest;
mod storage;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "ayeque-forge", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create or validate FORGE.toml in an existing directory.
    Init {
        /// Directory to initialize. Defaults to the current directory.
        path: Option<PathBuf>,
    },
    /// Resolve the nearest FORGE.toml and atomically replace FORGE.lock.
    Lock,
    /// Print the materialized path of a locked entity.
    Path {
        /// Entity id from the nearest FORGE.lock.
        id: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { path } => forge::init(path.as_deref()),
        Command::Lock => forge::lock(),
        Command::Path { id } => forge::path(&id),
    }
}
