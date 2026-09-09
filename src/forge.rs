use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::git::{
    ensure_checkout, ensure_repository, fetch_lfs, fetch_revision, materialize_lfs, resolve_tree,
};
use crate::manifest::{
    Manifest, Storage, parse_manifest, validate_artifact_path, validate_relative_path,
};
use crate::storage::{atomic_write, data_root, ensure_layout, sha256_hex};

const MANIFEST_NAME: &str = "FORGE.toml";
const LOCK_NAME: &str = "FORGE.lock";
const FORMAT: u32 = 1;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LockFile {
    format: u32,
    manifest_sha256: String,
    entity: Vec<LockEntity>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LockEntity {
    id: String,
    git: String,
    commit: String,
    path: String,
    tree: String,
}

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

    let directory = directory
        .canonicalize()
        .with_context(|| format!("failed to resolve {}", directory.display()))?;
    let manifest_path = directory.join(MANIFEST_NAME);
    let manifest = if manifest_path.exists() {
        let bytes = fs::read(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let manifest = parse_manifest(&bytes, &manifest_path)?;
        println!("already initialized {}", manifest_path.display());
        manifest
    } else {
        let manifest = Manifest {
            format: FORMAT,
            storage: None,
            entity: Vec::new(),
        };
        atomic_write(&manifest_path, b"format = 1\n")?;
        println!("created {}", manifest_path.display());
        manifest
    };

    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let storage = data_root_for(&directory, &manifest)?;
    ensure_layout(&storage)?;
    println!("storage {}", storage.display());

    let lock_path = directory.join(LOCK_NAME);
    if !lock_path.exists() {
        let lock = LockFile {
            format: FORMAT,
            manifest_sha256: format!("sha256:{}", sha256_hex(&manifest_bytes)),
            entity: Vec::new(),
        };
        write_lock(&lock_path, &lock)?;
        println!("created {}", lock_path.display());
    } else {
        println!("preserved {}", lock_path.display());
    }
    Ok(())
}

pub fn validate() -> Result<()> {
    let (manifest_path, _workspace, manifest) = current_manifest()?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let lock_path = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?
        .join(LOCK_NAME);
    let lock_bytes = fs::read(&lock_path).with_context(|| {
        format!(
            "failed to read {}; run `ayeque-forge init` first",
            lock_path.display()
        )
    })?;
    let lock = parse_lock(&lock_bytes, &lock_path)?;
    validate_lock_matches(
        &manifest,
        &manifest_bytes,
        &lock,
        &lock_path,
        &manifest_path,
    )?;
    println!("validated {}", manifest_path.display());
    println!("validated {}", lock_path.display());
    Ok(())
}

pub fn configure_storage_root(root: &Path) -> Result<()> {
    let (manifest_path, workspace, mut manifest) = current_manifest()?;
    let root = root
        .to_str()
        .ok_or_else(|| anyhow!("storage root must be valid UTF-8"))?;
    validate_artifact_path(root)?;
    manifest.storage = Some(Storage {
        root: root.to_owned(),
    });
    write_manifest(&manifest_path, &manifest)?;
    let data_root = data_root_for(&workspace, &manifest)?;
    ensure_layout(&data_root)?;
    println!("configured storage {}", data_root.display());
    Ok(())
}

pub fn configure_artifact_path(id: &str, path: &Path) -> Result<()> {
    ensure!(!id.is_empty(), "entity id must not be empty");
    let (manifest_path, _workspace, mut manifest) = current_manifest()?;
    let path = path
        .to_str()
        .ok_or_else(|| anyhow!("entity artifact path must be valid UTF-8"))?;
    validate_artifact_path(path)?;
    let entity = manifest
        .entity
        .iter_mut()
        .find(|entity| entity.id == id)
        .ok_or_else(|| {
            anyhow!(
                "entity {:?} is not present in {}",
                id,
                manifest_path.display()
            )
        })?;
    entity.artifact = Some(path.to_owned());
    write_manifest(&manifest_path, &manifest)?;
    println!("configured artifact {} {}", id, path);
    Ok(())
}

fn current_manifest() -> Result<(PathBuf, PathBuf, Manifest)> {
    let current = env::current_dir().context("failed to read the current directory")?;
    let manifest_path = find_manifest(&current)?;
    let workspace = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?
        .to_path_buf();
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest = parse_manifest(&bytes, &manifest_path)?;
    Ok((manifest_path, workspace, manifest))
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let serialized = toml::to_string_pretty(manifest).context("failed to serialize FORGE.toml")?;
    atomic_write(path, serialized.as_bytes())
}

pub fn lock() -> Result<()> {
    let current = env::current_dir().context("failed to read the current directory")?;
    let manifest_path = find_manifest(&current)?;
    let workspace = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?;
    let bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let mut manifest = parse_manifest(&bytes, &manifest_path)?;
    let data_root = data_root_for(workspace, &manifest)?;
    ensure_layout(&data_root)?;

    manifest.entity.sort_by(|a, b| a.id.cmp(&b.id));
    let mut repositories = BTreeMap::<String, PathBuf>::new();
    let mut locked = Vec::with_capacity(manifest.entity.len());

    for entity in manifest.entity {
        let source_hash = sha256_hex(entity.git.as_bytes());
        let repository = match repositories.get(&entity.git) {
            Some(path) => path.clone(),
            None => {
                let path = ensure_repository(&data_root, &source_hash, &entity.git)?;
                repositories.insert(entity.git.clone(), path.clone());
                path
            }
        };
        let commit = fetch_revision(&repository, &entity.git, &entity.revision)
            .with_context(|| format!("failed to resolve entity {:?}", entity.id))?;
        let tree = resolve_tree(&repository, &commit, &entity.path)
            .with_context(|| format!("invalid tree for entity {:?}", entity.id))?;
        fetch_lfs(&repository, &commit, &entity.path, &tree.lfs)?;
        let checkout = ensure_checkout(&data_root, &source_hash, &commit, &repository)?;
        materialize_lfs(&checkout, &entity.path, &tree.lfs)?;
        let checkout_entity = checkout.join(&entity.path);
        ensure!(
            checkout_entity.is_dir(),
            "materialized entity {:?} is not a directory",
            entity.id
        );
        let materialized = match entity.artifact.as_deref() {
            Some(path) => materialize_artifact(workspace, path, &checkout_entity)?,
            None => checkout_entity,
        };

        println!("{}\t{}", entity.id, materialized.display());
        locked.push(LockEntity {
            id: entity.id,
            git: entity.git,
            commit,
            path: entity.path,
            tree: tree.id,
        });
    }

    let lock = LockFile {
        format: FORMAT,
        manifest_sha256: format!("sha256:{}", sha256_hex(&bytes)),
        entity: locked,
    };
    let serialized = toml::to_string_pretty(&lock).context("failed to serialize FORGE.lock")?;
    atomic_write(&workspace.join(LOCK_NAME), serialized.as_bytes())?;
    Ok(())
}

pub fn path(id: &str) -> Result<()> {
    ensure!(!id.is_empty(), "entity id must not be empty");
    let current = env::current_dir().context("failed to read the current directory")?;
    let manifest_path = find_manifest(&current)?;
    let workspace = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest = parse_manifest(&manifest_bytes, &manifest_path)?;

    let lock_path = workspace.join(LOCK_NAME);
    let lock_bytes = fs::read(&lock_path).with_context(|| {
        format!(
            "failed to read {}; run `ayeque-forge lock` first",
            lock_path.display()
        )
    })?;
    let lock = parse_lock(&lock_bytes, &lock_path)?;
    let expected_digest = format!("sha256:{}", sha256_hex(&manifest_bytes));
    ensure!(
        lock.manifest_sha256 == expected_digest,
        "{} is stale; run `ayeque-forge lock`",
        lock_path.display()
    );
    ensure!(
        lock.entity.len() == manifest.entity.len(),
        "{} does not match {}; run `ayeque-forge lock`",
        lock_path.display(),
        manifest_path.display()
    );

    for locked in &lock.entity {
        let declared = manifest
            .entity
            .iter()
            .find(|entity| entity.id == locked.id)
            .ok_or_else(|| {
                anyhow!(
                    "{} contains undeclared entity {:?}; run `ayeque-forge lock`",
                    lock_path.display(),
                    locked.id
                )
            })?;
        ensure!(
            locked.git == declared.git && locked.path == declared.path,
            "locked entity {:?} does not match {}; run `ayeque-forge lock`",
            locked.id,
            manifest_path.display()
        );
    }

    let entity = lock
        .entity
        .iter()
        .find(|entity| entity.id == id)
        .ok_or_else(|| anyhow!("entity {:?} is not present in {}", id, lock_path.display()))?;
    let data_root = data_root_for(workspace, &manifest)?;
    let materialized = resolved_entity_path(workspace, &data_root, &manifest, entity)?;
    println!("{}", materialized.display());
    Ok(())
}

pub fn paths() -> Result<()> {
    let current = env::current_dir().context("failed to read the current directory")?;
    let manifest_path = find_manifest(&current)?;
    let workspace = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", manifest_path.display()))?;
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest = parse_manifest(&manifest_bytes, &manifest_path)?;
    let data_root = data_root_for(workspace, &manifest)?;

    println!("workspace	{}", workspace.display());
    println!("manifest	{}", manifest_path.display());
    println!("lock	{}", workspace.join(LOCK_NAME).display());
    println!("storage	{}", data_root.display());
    println!("git-cache	{}", data_root.join("git").display());
    println!("checkouts	{}", data_root.join("checkouts").display());

    let lock_path = workspace.join(LOCK_NAME);
    if !lock_path.is_file() {
        return Ok(());
    }
    let lock_bytes =
        fs::read(&lock_path).with_context(|| format!("failed to read {}", lock_path.display()))?;
    let lock = parse_lock(&lock_bytes, &lock_path)?;
    let expected_digest = format!("sha256:{}", sha256_hex(&manifest_bytes));
    ensure!(
        lock.manifest_sha256 == expected_digest,
        "{} is stale; run ayeque-forge lock",
        lock_path.display()
    );
    for entity in &lock.entity {
        let materialized = resolved_entity_path(workspace, &data_root, &manifest, entity)?;
        println!("entity.{}	{}", entity.id, materialized.display());
    }
    Ok(())
}

fn data_root_for(workspace: &Path, manifest: &Manifest) -> Result<PathBuf> {
    match manifest
        .storage
        .as_ref()
        .map(|storage| Path::new(&storage.root))
    {
        Some(root) if root.is_absolute() => Ok(root.to_path_buf()),
        Some(root) => Ok(workspace.join(root)),
        None => data_root(),
    }
}

fn artifact_path(workspace: &Path, configured: &str) -> PathBuf {
    let configured = Path::new(configured);
    if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        workspace.join(configured)
    }
}

fn materialize_artifact(workspace: &Path, configured: &str, source: &Path) -> Result<PathBuf> {
    let destination = artifact_path(workspace, configured);
    if let (Ok(source), Ok(destination)) = (source.canonicalize(), destination.canonicalize()) {
        ensure!(
            source != destination,
            "entity artifact path must differ from its managed checkout"
        );
    }
    if destination.exists() {
        ensure!(
            destination.is_dir(),
            "entity artifact path {} is not a directory",
            destination.display()
        );
    } else {
        fs::create_dir_all(&destination)
            .with_context(|| format!("failed to create {}", destination.display()))?;
    }
    copy_tree(source, &destination)?;
    Ok(destination)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("failed to read {}", source.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", source.display()))?;
        let source_path = entry.path();
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

fn materialized_path(data_root: &Path, entity: &LockEntity) -> Result<PathBuf> {
    let materialized = data_root
        .join("checkouts")
        .join(sha256_hex(entity.git.as_bytes()))
        .join(&entity.commit)
        .join(&entity.path)
        .canonicalize()
        .with_context(|| {
            format!(
                "locked entity {:?} is not materialized; run ayeque-forge lock",
                entity.id
            )
        })?;
    ensure!(
        materialized.is_dir(),
        "locked entity {:?} is not a directory; run ayeque-forge lock",
        entity.id
    );
    Ok(materialized)
}

fn resolved_entity_path(
    workspace: &Path,
    data_root: &Path,
    manifest: &Manifest,
    entity: &LockEntity,
) -> Result<PathBuf> {
    let declared = manifest
        .entity
        .iter()
        .find(|declared| declared.id == entity.id)
        .ok_or_else(|| anyhow!("entity {:?} is not declared", entity.id))?;
    if let Some(path) = declared.artifact.as_deref() {
        let path = artifact_path(workspace, path);
        ensure!(
            path.is_dir(),
            "entity {:?} artifact is not materialized; run ayeque-forge lock",
            entity.id
        );
        Ok(path)
    } else {
        materialized_path(data_root, entity)
    }
}

fn write_lock(path: &Path, lock: &LockFile) -> Result<()> {
    let serialized = toml::to_string_pretty(lock).context("failed to serialize FORGE.lock")?;
    atomic_write(path, serialized.as_bytes())
}

fn validate_lock_matches(
    manifest: &Manifest,
    manifest_bytes: &[u8],
    lock: &LockFile,
    lock_path: &Path,
    manifest_path: &Path,
) -> Result<()> {
    let expected_digest = format!("sha256:{}", sha256_hex(manifest_bytes));
    ensure!(
        lock.manifest_sha256 == expected_digest,
        "{} is stale; run `ayeque-forge lock`",
        lock_path.display()
    );
    ensure!(
        lock.entity.len() == manifest.entity.len(),
        "{} does not match {}; run `ayeque-forge lock`",
        lock_path.display(),
        manifest_path.display()
    );
    for locked in &lock.entity {
        let declared = manifest
            .entity
            .iter()
            .find(|entity| entity.id == locked.id)
            .ok_or_else(|| {
                anyhow!(
                    "{} contains undeclared entity {:?}; run `ayeque-forge lock`",
                    lock_path.display(),
                    locked.id
                )
            })?;
        ensure!(
            locked.git == declared.git && locked.path == declared.path,
            "locked entity {:?} does not match {}; run `ayeque-forge lock`",
            locked.id,
            manifest_path.display()
        );
    }
    Ok(())
}

fn parse_lock(bytes: &[u8], path: &Path) -> Result<LockFile> {
    let source =
        std::str::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    let lock: LockFile =
        toml::from_str(source).with_context(|| format!("failed to parse {}", path.display()))?;
    ensure!(
        lock.format == FORMAT,
        "unsupported FORGE.lock format {}",
        lock.format
    );
    let mut ids = std::collections::BTreeSet::new();
    for entity in &lock.entity {
        ensure!(!entity.id.is_empty(), "locked entity id must not be empty");
        ensure!(
            ids.insert(entity.id.as_str()),
            "duplicate locked entity id {:?}",
            entity.id
        );
        validate_object_id(&entity.commit, "commit")?;
        validate_object_id(&entity.tree, "tree")?;
        validate_relative_path(&entity.path)?;
    }
    Ok(lock)
}

fn validate_object_id(value: &str, name: &str) -> Result<()> {
    ensure!(
        matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "locked entity {name} is not a Git object id"
    );
    Ok(())
}

fn find_manifest(start: &Path) -> Result<PathBuf> {
    for directory in start.ancestors() {
        let candidate = directory.join(MANIFEST_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    bail!(
        "no {} found in {} or its parents",
        MANIFEST_NAME,
        start.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_serialization_is_stable() {
        let lock = LockFile {
            format: 1,
            manifest_sha256: "sha256:abc".into(),
            entity: vec![LockEntity {
                id: "a".into(),
                git: "/repo".into(),
                commit: "commit".into(),
                path: "entity".into(),
                tree: "tree".into(),
            }],
        };
        assert_eq!(
            toml::to_string_pretty(&lock).unwrap(),
            "format = 1\nmanifest_sha256 = \"sha256:abc\"\n\n[[entity]]\nid = \"a\"\ngit = \"/repo\"\ncommit = \"commit\"\npath = \"entity\"\ntree = \"tree\"\n"
        );
    }
}
