use std::collections::BTreeSet;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

const FORMAT: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    pub(crate) format: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) storage: Option<Storage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) entity: Vec<ManifestEntity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestEntity {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) schema: String,
    pub(crate) git: String,
    pub(crate) revision: String,
    pub(crate) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) artifact: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Storage {
    pub(crate) root: String,
}

pub(crate) fn parse_manifest(bytes: &[u8], path: &Path) -> Result<Manifest> {
    let source =
        std::str::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    let manifest: Manifest =
        toml::from_str(source).with_context(|| format!("failed to parse {}", path.display()))?;
    ensure!(
        manifest.format == FORMAT,
        "unsupported FORGE.toml format {}",
        manifest.format
    );
    if let Some(storage) = &manifest.storage {
        ensure!(!storage.root.is_empty(), "storage root must not be empty");
        ensure!(
            !storage.root.contains(['\0', '\n', '\r']),
            "storage root contains an invalid character"
        );
    }

    let mut ids = BTreeSet::new();
    for entity in &manifest.entity {
        validate_entity(entity)?;
        ensure!(
            ids.insert(entity.id.as_str()),
            "duplicate entity id {:?}",
            entity.id
        );
    }
    Ok(manifest)
}

fn validate_entity(entity: &ManifestEntity) -> Result<()> {
    for (name, value) in [
        ("id", entity.id.as_str()),
        ("kind", entity.kind.as_str()),
        ("schema", entity.schema.as_str()),
        ("git", entity.git.as_str()),
        ("revision", entity.revision.as_str()),
        ("path", entity.path.as_str()),
    ] {
        ensure!(!value.is_empty(), "entity {} must not be empty", name);
        ensure!(
            !value.contains(['\0', '\n', '\r']),
            "entity {} contains an invalid character",
            name
        );
    }
    ensure!(
        !entity.git.starts_with('-'),
        "entity git must not start with '-'"
    );
    ensure!(
        !entity.revision.starts_with('-'),
        "entity revision must not start with '-'"
    );
    validate_git_source(&entity.git)?;
    validate_relative_path(&entity.path)?;
    if let Some(artifact) = &entity.artifact {
        validate_artifact_path(artifact)?;
    }
    Ok(())
}

fn validate_git_source(source: &str) -> Result<()> {
    if Path::new(source).is_absolute() || source.contains("://") {
        return Ok(());
    }
    let colon = source.find(':');
    let slash = source.find('/');
    if colon.is_some_and(|colon| slash.is_none_or(|slash| colon < slash)) {
        return Ok(());
    }
    bail!("entity git must be a URL, scp-like SSH source, or absolute local path")
}

pub(crate) fn validate_relative_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    ensure!(!path.is_absolute(), "entity path must be relative");
    let mut saw_normal = false;
    for component in path.components() {
        match component {
            Component::Normal(_) => saw_normal = true,
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("entity path must not contain '..' or a root component")
            }
        }
    }
    ensure!(saw_normal || value == ".", "entity path must not be empty");
    Ok(())
}

pub(crate) fn validate_artifact_path(value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "entity artifact path must not be empty");
    ensure!(
        !value.contains(['\0', '\n', '\r']),
        "entity artifact path contains an invalid character"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_manifest_is_valid() {
        let manifest = parse_manifest(b"format = 1\n", Path::new("FORGE.toml")).unwrap();
        assert!(manifest.entity.is_empty());
        assert!(manifest.storage.is_none());
    }

    #[test]
    fn manifest_rejects_unknown_fields() {
        let error =
            parse_manifest(b"format = 1\nextra = true\n", Path::new("FORGE.toml")).unwrap_err();
        assert!(error.to_string().contains("failed to parse"));
    }

    #[test]
    fn manifest_rejects_duplicate_ids() {
        let source = br#"
format = 1

[[entity]]
id = "same"
kind = "one"
schema = "1"
git = "/tmp/repo"
revision = "main"
path = "first"

[[entity]]
id = "same"
kind = "two"
schema = "1"
git = "/tmp/repo"
revision = "main"
path = "second"
"#;
        let error = parse_manifest(source, Path::new("FORGE.toml")).unwrap_err();
        assert!(error.to_string().contains("duplicate entity id"));
    }

    #[test]
    fn path_validation_rejects_parent_components() {
        assert!(validate_relative_path("a/../b").is_err());
        assert!(validate_relative_path("/absolute").is_err());
        assert!(validate_relative_path("entity").is_ok());
        assert!(validate_relative_path(".").is_ok());
    }

    #[test]
    fn storage_root_accepts_absolute_and_relative_paths() {
        for root in ["/tmp/forge-data", ".forge-data", "../forge-data"] {
            let source = format!("format = 1\n[storage]\nroot = \"{root}\"\n");
            assert!(parse_manifest(source.as_bytes(), Path::new("FORGE.toml")).is_ok());
        }
    }
}
