use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use ayeque_forge_core as core;

use crate::storage::atomic_write;

const MANIFEST_NAME: &str = "FORGE.toml";
const LOCK_NAME: &str = "FORGE.lock";
const FORMAT: u32 = 2;

pub fn init(path: Option<&Path>) -> Result<()> {
    let directory = match path {
        Some(path) => path.to_path_buf(),
        None => env::current_dir().context("failed to read the current directory")?,
    };
    ensure!(
        directory.is_dir(),
        "{} is not an existing directory",
        directory.display()
    );
    let storage = core::data_root()?;
    let location = core::initialize_project(&storage, &directory)?;
    let manifest_path = location.project_path().join(MANIFEST_NAME);
    if !manifest_path.exists() {
        atomic_write(&manifest_path, format!("format = {FORMAT}\n").as_bytes())?;
        println!("created {}", manifest_path.display());
    } else {
        let bytes = fs::read(&manifest_path)?;
        core::parse_manifest(&bytes, &manifest_path)?;
        println!("already initialized {}", manifest_path.display());
    }
    let lock_path = location.project_path().join(LOCK_NAME);
    if !lock_path.exists() {
        let project = core::resolve_project_at(&storage, &directory)?;
        atomic_write(&lock_path, &core::serialize_lock(&project, Vec::new())?)?;
        println!("created {}", lock_path.display());
    } else {
        println!("preserved {}", lock_path.display());
    }
    println!("project {}", location.project_path().display());
    Ok(())
}

pub fn validate() -> Result<()> {
    let project = current_project()?;
    let lock = core::verify_lock(&project)?;
    println!("validated {}", project.manifest_path().display());
    println!("validated {}", project.lock_path().display());
    let _ = lock;
    Ok(())
}

pub fn configure_storage_root(root: &Path) -> Result<()> {
    ensure!(root.is_absolute(), "storage root override must be absolute");
    let _ = core::write_storage_root(root)?;
    core::ensure_layout(root)?;
    println!("configured storage {}", root.display());
    Ok(())
}

pub fn lock() -> Result<()> {
    let project = current_project()?;
    let refreshed = core::refresh_lock(&project)?;
    for entity in refreshed.manifest().entities() {
        let resolved = core::resolve_entity(&refreshed, entity.id())?;
        println!(
            "{}\t{}",
            entity.id(),
            resolved.materialized_path().display()
        );
    }
    Ok(())
}

pub fn path(id: &str) -> Result<()> {
    ensure!(!id.is_empty(), "entity id must not be empty");
    let project = current_project()?;
    let entity = core::resolve_entity(&project, id)?;
    println!("{}", entity.materialized_path().display());
    Ok(())
}

pub fn paths() -> Result<()> {
    let project = current_project()?;
    let lock = core::verify_lock(&project)?;
    println!("workspace\t{}", project.workspace().display());
    println!("project\t{}", project.project_path().display());
    println!("manifest\t{}", project.manifest_path().display());
    println!("lock\t{}", project.lock_path().display());
    println!("storage\t{}", project.storage_root().display());
    println!(
        "git-cache\t{}",
        project.storage_root().join("git").display()
    );
    println!(
        "projects\t{}",
        project.storage_root().join("projects").display()
    );
    println!("entities\t{}", project.entities_path().display());
    for entry in lock.entities() {
        let entity = core::resolve_entity(&project, entry.id())?;
        println!(
            "entity.{}\t{}",
            entity.id(),
            entity.materialized_path().display()
        );
    }
    Ok(())
}

fn current_project() -> Result<core::VerifiedProject> {
    let current = env::current_dir().context("failed to read the current directory")?;
    core::resolve_project(&current)
}
