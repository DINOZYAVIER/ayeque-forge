mod forge;
mod git;
mod storage;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "ayeque-forge",
    version,
    about = "Git-backed authoring and distribution for typed entities",
    long_about = "Git-backed authoring and distribution for typed entities.\n\nThe Forge manifest, lock, project identity, and materialized entities live in XDG storage by default; the source workspace remains untouched.",
    after_help = "Quick start:\n  ayeque-forge init\n  ayeque-forge validate\n  ayeque-forge lock\n  ayeque-forge paths\n  ayeque-forge path <ID>\n\nConfigure shared storage:\n  ayeque-forge config storage-root PATH\n\nEntity paths are printed from the XDG project catalog.\n"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize the XDG project catalog for a workspace.
    Init {
        /// Directory to initialize. Defaults to the current directory.
        path: Option<PathBuf>,
    },
    /// Validate the XDG FORGE.toml and FORGE.lock.
    Validate,
    /// Change global Forge configuration without reinitializing projects.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Resolve the XDG manifest and atomically replace lock and entities.
    Lock,
    /// Print the materialized XDG path of an entity.
    Path {
        /// Entity id from the current project's lock.
        id: String,
    },
    /// Show workspace, project, storage, cache, and materialized paths.
    Paths,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Set the global Git/cache and project storage root.
    StorageRoot {
        /// Absolute storage root.
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init { path } => forge::init(path.as_deref()),
        Command::Validate => forge::validate(),
        Command::Config {
            command: ConfigCommand::StorageRoot { path },
        } => forge::configure_storage_root(&path),
        Command::Lock => forge::lock(),
        Command::Path { id } => forge::path(&id),
        Command::Paths => forge::paths(),
    }
}
