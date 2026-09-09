# AGL-204: Forge workspace bootstrap and artifact paths

## Status

Implemented in ayeque-forge alpha. This specification defines the workspace
contract that agentLIBRE agents may rely on.

## Problem

agentLIBRE needs a reproducible way to declare Git-backed entities while
keeping downloaded repositories and materialized artifacts outside the
workspace. A new agent must be able to discover the complete workflow from
the CLI help and must not need to scan an implementation-specific cache.

## Goals

- Bootstrap a workspace with one command.
- Keep FORGE.toml and FORGE.lock at the workspace root.
- Store managed Git repositories and checkouts in XDG data storage by default.
- Allow a person to select an absolute or workspace-relative shared storage
  root in FORGE.toml.
- Allow each entity to select its own absolute or workspace-relative artifact
  directory.
- Expose stable commands for discovering all relevant paths.
- Give consumers one explicit materialized directory per entity.
- Keep the lockfile reviewable and suitable for Git.

## Non-goals

- Automatically adding or committing files to Git.
- Putting materialized artifacts in the repository by default.
- Making consumers understand the cache hash or checkout layout.
- Fetching data from path or paths.

## Workspace contract

Running:

    ayeque-forge init

in an existing directory MUST:

1. Create FORGE.toml if it does not exist, with format = 1.
2. Create FORGE.lock if it does not exist. For an empty manifest this is a
   valid lock with zero entities.
3. Create the storage root and its git/ and checkouts/ directories.
4. Leave existing valid manifest and lock content intact.
5. Fail without replacing an invalid existing FORGE.toml.

The user then adds the two root files explicitly:

    git add FORGE.toml FORGE.lock

init MUST NOT run git add or git commit.

## Storage contract

With no [storage] section, the storage root MUST be:

    $XDG_DATA_HOME/ayeque-forge

or, when XDG_DATA_HOME is unset:

    $HOME/.local/share/ayeque-forge

The root MUST contain:

    git/
    checkouts/

The optional manifest setting is:

    [storage]
    root = ".forge-data"

storage.root MUST accept both absolute paths and relative paths. A relative
root is resolved against the directory containing FORGE.toml, not against
the caller's current directory. The selected root is used consistently by
init, lock, path, and paths.

An entity MAY specify its own artifact directory:

    artifact = "../artifacts/agent"

The artifact path is resolved relative to the workspace when it is not
absolute. It is the path returned by path ID after a successful lock. The
shared storage root continues to hold Git mirrors and managed checkouts.

## CLI contract

ayeque-forge --help MUST expose:

- init [PATH]
- validate
- config storage-root PATH
- lock
- path <ID> [--change PATH]
- paths

config storage-root PATH MUST write or replace [storage].root in the nearest
workspace. It MUST accept both absolute and relative paths. Changing the
setting changes the manifest digest, so the next path or paths operation MUST
require a new lock.

path ID --change PATH MUST write or replace that entity's artifact field in
FORGE.toml. It MUST accept both absolute and relative paths. Changing the
setting changes the manifest digest, so the next path or paths operation MUST
require a new lock.

validate MUST parse FORGE.toml and FORGE.lock, verify their format and
manifest digest, and report failure without fetching or changing files.

lock MUST resolve declared revisions, materialize entity trees in the selected
storage root, and atomically replace the root FORGE.lock. A failed lock MUST
leave an existing lockfile unchanged.

path <ID> MUST:

- find the nearest workspace root;
- verify that FORGE.lock matches FORGE.toml;
- verify that the requested entity is locked and materialized;
- print exactly one absolute directory path to stdout;
- perform no fetch or mutation.

paths MUST print stable tab-separated name/path entries for:

- workspace
- manifest
- lock
- storage
- git-cache
- checkouts
- entity.<ID> for each locked entity

It is the human- and agent-facing diagnostic command. Consumers needing one
path SHOULD use path <ID> so command substitution cannot capture unrelated
status text.

## agentLIBRE usage

An agent bootstrapping an agentLIBRE Forge workspace can use:

    ayeque-forge --help
    ayeque-forge init
    ayeque-forge config storage-root /var/cache/agentlibre/forge
    git add FORGE.toml FORGE.lock
    ayeque-forge lock
    ayeque-forge paths
    ayeque-forge path agent --change ../artifacts/agent
    ayeque-forge lock
    entity_root="$(ayeque-forge path agent)"

The agent receives entity_root as an explicit input to AGL. It never derives
that path by scanning XDG directories.

## Acceptance criteria

- A clean temporary workspace can be initialized without a Git repository.
- Initialization creates root FORGE.toml, root FORGE.lock, and XDG git/ and
  checkouts/ directories.
- Re-running init is idempotent for a valid workspace.
- A configured relative root is resolved from the workspace.
- A configured absolute root is used as-is.
- config storage-root changes the shared root without using init as a setter.
- path ID --change configures one entity's artifact directory.
- lock and path use the configured root and per-entity artifact paths.
- paths reports all workspace and artifact locations.
- Editing FORGE.toml makes path and paths reject the stale lock.
- A failed lock does not overwrite the previous lock.
- cargo test and the CLI help output verify the contract.
