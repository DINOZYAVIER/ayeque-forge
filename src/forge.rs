use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use ayeque_forge_core as core;

use crate::git::{
    ensure_checkout, ensure_repository, fetch_lfs, fetch_revision, materialize_lfs, resolve_tree,
};
use crate::storage::{atomic_write, remove_internal_path_if_present};

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
    let data_root = project.storage_root().to_owned();
    let mut declarations = project.manifest().entities().to_vec();
    declarations.sort_by(|a, b| a.id().cmp(b.id()));
    let mut repositories = BTreeMap::<String, PathBuf>::new();
    let mut locked = Vec::with_capacity(declarations.len());
    let entities = project.entities_path().to_owned();
    let staged_entities = project
        .project_path()
        .join(format!(".entities.staging-{}", std::process::id()));
    remove_internal_path_if_present(&staged_entities, project.project_path())?;
    fs::create_dir_all(&staged_entities)?;

    for entity in declarations {
        let source_hash = core::sha256_hex(entity.git().as_bytes());
        let repository = match repositories.get(entity.git()) {
            Some(path) => path.clone(),
            None => {
                let path = ensure_repository(&data_root, &source_hash, entity.git())?;
                repositories.insert(entity.git().to_owned(), path.clone());
                path
            }
        };
        let commit = fetch_revision(&repository, entity.git(), entity.revision())
            .with_context(|| format!("failed to resolve entity {:?}", entity.id()))?;
        let tree = resolve_tree(&repository, &commit, entity.path())
            .with_context(|| format!("invalid tree for entity {:?}", entity.id()))?;
        fetch_lfs(&repository, &commit, entity.path(), &tree.lfs)?;
        let checkout = ensure_checkout(&data_root, &source_hash, &commit, &repository)?;
        materialize_lfs(&checkout, entity.path(), &tree.lfs)?;
        let checkout_entity = checkout.join(entity.path());
        ensure!(
            checkout_entity.is_dir(),
            "materialized entity {:?} is not a directory",
            entity.id()
        );
        let materialized = staged_entities.join(entity.id());
        fs::create_dir_all(&materialized)?;
        copy_tree(&checkout_entity, &materialized)?;
        locked.push(core::LockEntry::new(
            entity.id().to_owned(),
            entity.git().to_owned(),
            commit,
            entity.path().to_owned(),
            tree.id,
        )?);
    }

    let serialized = core::serialize_lock(&project, locked.clone())?;
    let old_entities = project
        .project_path()
        .join(format!(".entities.previous-{}", std::process::id()));
    remove_internal_path_if_present(&old_entities, project.project_path())?;
    if entities.exists() {
        fs::rename(&entities, &old_entities)?;
    }
    if let Err(error) = fs::rename(&staged_entities, &entities) {
        if old_entities.exists() {
            let _ = fs::rename(&old_entities, &entities);
        }
        return Err(error.into());
    }
    if old_entities.exists() {
        fs::remove_dir_all(&old_entities)?;
    }
    atomic_write(project.lock_path(), &serialized)?;
    for entity in &locked {
        println!("{}\t{}", entity.id(), entities.join(entity.id()).display());
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

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", source.display()))?;
        let source_path = entry.path();
        if source_path.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", source_path.display()))?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path)
                .with_context(|| format!("failed to create {}", destination_path.display()))?;
            copy_tree(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }
    Ok(())
}
