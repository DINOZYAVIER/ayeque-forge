use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};

use crate::git::{
    ensure_checkout, ensure_repository, fetch_lfs, fetch_revision, materialize_lfs, resolve_tree,
};
use crate::manifest::{parse_manifest, validate_relative_path};
use crate::storage::{atomic_write, data_root, sha256_hex};

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

    let manifest_path = directory.join(MANIFEST_NAME);
    if manifest_path.exists() {
        let bytes = fs::read(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        parse_manifest(&bytes, &manifest_path)?;
        println!("validated {}", manifest_path.display());
    } else {
        atomic_write(&manifest_path, b"format = 1\n")?;
        println!("created {}", manifest_path.display());
    }
    Ok(())
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
    let data_root = data_root()?;

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
        let materialized = checkout.join(&entity.path);
        ensure!(
            materialized.is_dir(),
            "materialized entity {:?} is not a directory",
            entity.id
        );

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
    let materialized = data_root()?
        .join("checkouts")
        .join(sha256_hex(entity.git.as_bytes()))
        .join(&entity.commit)
        .join(&entity.path);
    let materialized = materialized.canonicalize().with_context(|| {
        format!(
            "locked entity {:?} is not materialized; run `ayeque-forge lock`",
            id
        )
    })?;
    ensure!(
        materialized.is_dir(),
        "locked entity {:?} is not a directory; run `ayeque-forge lock`",
        id
    );
    println!("{}", materialized.display());
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
