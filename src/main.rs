mod forge;
mod git;
mod manifest;
mod storage;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "ayeque-forge",
    version,
    about = "Git-backed authoring and distribution for typed entities",
    long_about = "Git-backed authoring and distribution for typed entities.\n\n\
The workspace keeps FORGE.toml and FORGE.lock at its root. Git mirrors and\n\
managed checkouts live in XDG storage by default; entities may opt into an\n\
absolute or workspace-relative artifact path.",
    after_help = r#"Quick start:
  ayeque-forge init
  ayeque-forge validate
  git add FORGE.toml FORGE.lock
  ayeque-forge lock
  ayeque-forge paths
  ayeque-forge path <ID>

Configure shared storage:
  ayeque-forge config storage-root PATH

Configure one entity's artifact directory:
  ayeque-forge path <ID> --change PATH
  ayeque-forge lock

Relative paths are resolved from the directory containing FORGE.toml.
"#
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Initialize a workspace, its storage directories, and FORGE.lock.
    ///
    /// Existing FORGE.toml and FORGE.lock files are preserved. Use
    /// `config storage-root` or `path ID --change PATH` to change paths.
    Init {
        /// Directory to initialize. Defaults to the current directory.
        path: Option<PathBuf>,
    },
    /// Validate FORGE.toml and FORGE.lock without fetching or changing files.
    Validate,
    /// Change workspace configuration without reinitializing the workspace.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Resolve the nearest FORGE.toml and atomically replace FORGE.lock.
    Lock,
    /// Print or configure the materialized path of an entity.
    Path {
        /// Entity id from the nearest FORGE.lock.
        id: String,
        /// Set this entity's materialized artifact path in FORGE.toml.
        ///
        /// The path may be absolute or relative to the workspace. This
        /// changes the manifest and requires `ayeque-forge lock`.
        #[arg(long, value_name = "PATH")]
        change: Option<PathBuf>,
    },
    /// Show the workspace, lock, storage, cache, and materialized paths.
    ///
    /// Output is tab-separated name/path pairs for agents and diagnostics.
    Paths,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Set the shared Git/cache storage root in FORGE.toml.
    ///
    /// The path may be absolute or relative to the workspace. Changing it
    /// makes FORGE.lock stale until `ayeque-forge lock` is run.
    StorageRoot {
        /// Absolute path or path relative to the workspace.
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
        Command::Path { id, change } => match change {
            Some(path) => forge::configure_artifact_path(&id, &path),
            None => forge::path(&id),
        },
        Command::Paths => forge::paths(),
    }
}
