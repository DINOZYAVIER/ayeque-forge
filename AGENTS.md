# Agent instructions

## Project purpose

Ayeque Forge is the Git-backed authoring and distribution layer for typed
entities. The CLI binary is ayeque-forge.

## Forge workspace workflow

Start by checking the complete CLI contract:

    ayeque-forge --help

Initialize an existing workspace directory:

    ayeque-forge init

This initializes FORGE.toml, creates FORGE.lock when absent, and creates the
default storage layout:

    $XDG_DATA_HOME/ayeque-forge/git
    $XDG_DATA_HOME/ayeque-forge/checkouts

If XDG_DATA_HOME is unset, the fallback is
$HOME/.local/share/ayeque-forge. The workspace root should contain and
version FORGE.toml and FORGE.lock:

    git add FORGE.toml FORGE.lock

init does not edit an existing manifest and does not run git add or git
commit.

Validate the manifest and lockfile explicitly without fetching:

    ayeque-forge validate

Resolve declared entities and update the lock:

    ayeque-forge lock

Inspect all known locations:

    ayeque-forge paths

Resolve one entity for a consumer:

    entity_root="$(ayeque-forge path agent)"

The path command prints only the absolute directory path and never fetches
data.

## Changing storage paths

The shared Git/cache root is configured separately from initialization:

    ayeque-forge config storage-root /var/cache/agentlibre/forge
    ayeque-forge config storage-root ../forge-data
    ayeque-forge lock

Relative paths are resolved from the directory containing FORGE.toml.

An individual entity can use a separate artifact directory:

    ayeque-forge path agent --change /srv/agentlibre/agent
    ayeque-forge lock
    ayeque-forge path agent

This writes artifact = ... to that entity in FORGE.toml. The next
ayeque-forge lock copies the materialized entity into the configured artifact
directory. Until then, path and paths reject the stale lock.

## Development commands

Build the release binary:

    scripts/build-ayeque-forge.sh

Change one entity's artifact path through the repository helper:

    scripts/ayeque-forge-path.sh agent /srv/agentlibre/agent

The helper accepts an optional third argument for a specific binary:

    scripts/ayeque-forge-path.sh agent /srv/agentlibre/agent \
      target/debug/ayeque-forge

Run verification before handing off changes:

    cargo fmt --check
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
    scripts/live-smoke.sh target/debug/ayeque-forge

## Manifest conventions

- Use uppercase FORGE.toml and FORGE.lock at the workspace root.
- Entity path is the source directory inside the Git revision.
- Entity artifact is an optional absolute or workspace-relative output
  directory.
- Keep materialized artifacts outside the repository unless the workspace
  explicitly chooses a relative storage or artifact path.
- A changed manifest requires ayeque-forge lock before paths are consumed.
