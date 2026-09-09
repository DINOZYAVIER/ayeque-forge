# 0002: XDG-owned Forge projects

## Status

Planned specification. The human selected this direction on 2026-09-09.
Implementation is not authorized here. The intended implementer is Luna
Medium.

## Human direction

- A source repository contains no Forge files or materialized Forge data.
- Forge owns every project manifest, lock, identity record and materialized
  entity below its XDG roots.
- The default project name is the source repository/directory basename. A
  deterministic unique suffix distinguishes equal basenames.
- A whole-repository entity omits `path`; explicit `path` remains available
  only for a source subdirectory.
- Keep one global storage override. Remove per-entity destinations and parallel
  materialization layouts.

## Current implementation

Current `main` does the opposite:

- `src/forge.rs::find_manifest` searches the current directory and parents for
  repository-local `FORGE.toml`;
- `init` writes `FORGE.toml` and `FORGE.lock` into that source workspace;
- `src/manifest.rs::ManifestEntity::path` is required;
- XDG contains shared `git/` plus hashed
  `checkouts/<source-hash>/<commit>/<path>`;
- optional `artifact` and `path ID --change PATH` create a second destination;
- live XDG also has manually maintained root `entities/` and `authoring/`
  zones, but no `projects/` catalog.

`materialize_artifact` recursively merges into an existing directory without
stale-file removal or atomic replacement. It can damage an authoring checkout
if configured as that destination. The target removes this mode.

## XDG ownership and layout

Defaults:

```text
config: ${XDG_CONFIG_HOME:-$HOME/.config}/ayeque-forge/config.toml
data:   ${XDG_DATA_HOME:-$HOME/.local/share}/ayeque-forge/
```

Data layout:

```text
ayeque-forge/
├── git/
└── projects/
    ├── agentLIBRE/
    │   ├── project.toml
    │   ├── FORGE.toml
    │   ├── FORGE.lock
    │   └── entities/
    │       ├── chat/
    │       └── chat-agent/
    └── agentLIBRE-a91c42b337ef/
        └── ...
```

The source workspace contains no Forge manifest, lock, marker, symlink,
generated directory or ignore entry. `git/` remains the shared object cache.
The active format does not create or read root `checkouts/`, `entities/`, or
`authoring/`. Existing live directories are not automatically deleted.

## Project discovery and collision handling

A project is associated with one canonical source workspace. Commands use the
nearest Git root (a `.git` directory or worktree `.git` file); outside Git they
use the exact current directory. `init [PATH]` applies the same rule to PATH.

Forge computes `workspace_sha256` from the canonical absolute workspace path
encoded as platform path bytes. The usual project key is its basename.
`project.toml` contains:

```toml
format = 1
workspace_sha256 = "sha256:<64 lowercase hex>"
```

Selection:

1. Claim `projects/<basename>` when absent.
2. Reuse it when its marker has the same identity.
3. On a different identity, use
   `<basename>-<first 12 hex characters of workspace_sha256>`.
4. If that also conflicts, extend to the full digest; if it still conflicts,
   fail without mutation.

The marker stores no raw workspace path. Moving a workspace creates a new
identity/project; Forge does not move or delete the old project implicitly.
There is no project-name setting in a source repository.

## XDG project manifest

`projects/<project-key>/FORGE.toml` uses strict `format = 2`:

```toml
format = 2

[[entity]]
id = "tasks"
kind = "tasks"
schema = "1"
git = "ssh://host/repository.git"
revision = "main"
```

`path` is optional and defaults to `.`. It selects a directory inside the Git
revision, so a monorepo may explicitly use `path = "functions/chat"`.

Remove `artifact`; format 2 rejects it. Remove `path ID --change PATH` and
`configure_artifact_path`. Every destination is
`projects/<project-key>/entities/<entity-id>`.

Entity IDs must be safe single path components: reject empty, `.`, `..`, `/`,
NUL, newline and carriage return. Letters, digits, `.`, `_`, `-`, and `:` are
valid. Duplicate IDs remain invalid.

`FORGE.lock` format 2 stores `project_key` and `workspace_sha256` plus each
locked source identity. All read commands require the lock, current workspace
identity, project marker and manifest digest to agree.

## Global override

The only destination override is the global XDG config:

```toml
format = 1
storage_root = "/srv/ayeque-forge"
```

`storage_root` is absolute and relocates `git/` and `projects/` together. An
absent config uses the XDG data default. `ayeque-forge config storage-root`
writes it atomically. Changing it selects another complete catalog; it does not
move data. Repository-relative storage and per-entity destinations are gone.

## CLI contract

```text
ayeque-forge init [PATH]
ayeque-forge validate
ayeque-forge config storage-root PATH
ayeque-forge lock
ayeque-forge path <ID>
ayeque-forge paths
```

`init` claims the XDG project and creates valid `project.toml`, format-2
manifest, empty matching lock and `entities/` when absent. It prints the XDG
project path and never writes the source workspace. Humans edit that XDG
manifest.

Other commands resolve the project from the current workspace. They fail if it
has not been initialized. `validate`, `path`, and `paths` do not fetch, repair,
or mutate.

`path <ID>` prints only:

```text
<storage>/projects/<project-key>/entities/<entity-id>
```

`paths` prints tab-separated `workspace`, `config`, `storage`, `git-cache`,
`projects`, `project`, `manifest`, `lock`, `entities`, and each `entity.<id>`.
It does not report `checkouts`.

## Lock transaction

`lock` resolves and verifies every revision, source tree and LFS object before
changing the current entity tree. It then:

1. materializes the complete next `entities/` tree in a sibling project-local
   staging directory;
2. records the prospective manifest digest in transaction metadata;
3. renames current `entities/` to a bounded backup and staging to `entities/`;
4. atomically replaces `FORGE.lock` last;
5. restores the prior tree and lock on a reported commit error;
6. removes the backup after success.

Tree and lock cannot change in one filesystem rename. A crash between them may
leave a new tree and old lock, so each generation carries its manifest digest.
Read commands reject a mismatch; the next `lock` reconstructs the complete
tree. Do not claim cross-file crash atomicity.

Removing a declaration removes its entity only with the complete transaction.
Cleanup may touch only transaction-owned names inside the resolved XDG project,
never the source workspace, storage root or another project.

## Implementation map

- `src/manifest.rs`: format 2; optional source `path`; remove `artifact`; safe
  entity IDs; strict project marker/global-config types.
- `src/storage.rs`: independent XDG config/data roots; `git/` and `projects/`;
  workspace identity, collision resolution and bounded transaction helpers.
- `src/forge.rs`: replace `find_manifest` with XDG project resolution; make
  `init` XDG-only; update lock; replace the whole entities tree; remove artifact
  configuration and merge-copy code.
- `src/main.rs`: remove `path --change`; replace repository-local wording with
  XDG project behavior.
- `README.md`, `AGENTS.md`, and
  `authoring/agentLIBRE/AGL-204-forge-workspace-cli.md`: replace the old
  repository-local contract.
- Remove this repository's tracked `FORGE.toml` and `FORGE.lock`; add no
  replacement source-repository file.

This is a breaking alpha cutover: no format-1 reader, migration shim,
repository-local fallback, destination alias, per-entity override, registry
daemon, or database.

## Verification

Tests must prove:

- `init` leaves every source-workspace byte and directory entry unchanged;
- all Forge state exists only below the selected XDG roots;
- omitted `path` locks a repository root and explicit source subdirectory
  `path` works;
- `artifact`, format 1 and `path --change` are rejected;
- equal workspace basenames receive isolated deterministic project keys;
- Git child/root resolution agrees; a non-Git directory resolves only itself;
- moving a workspace creates a new project without mutating the old one;
- malformed markers, stale locks and identity mismatches fail without writes;
- entity IDs cannot escape `entities/`;
- revision replacement removes stale files;
- resolve, LFS, staging and reported commit failures preserve the prior tree
  and lock;
- global storage override selects a separate complete catalog;
- `paths` reports XDG project locations and no checkout directory.

Run `cargo fmt --check`, `cargo test`,
`cargo clippy --all-targets --all-features -- -D warnings`, and
`scripts/live-smoke.sh target/debug/ayeque-forge`.

The live smoke creates two temporary Git repositories with the same basename,
confirms neither receives Forge files, locks root and subdirectory entities,
changes a revision, and verifies XDG isolation and stale-file removal.

## Luna Medium handoff constraints

- Preserve the existing untracked `authoring/ayeque-forge/` work.
- Do not run old `lock` with `artifact` aimed at an authoring checkout.
- Do not delete live XDG `checkouts/`, root `entities/`, or `authoring/` without
  separate explicit authorization.
- Do not mutate other source repositories during this cutover.
- Do not commit or push unless the human separately requests it.
