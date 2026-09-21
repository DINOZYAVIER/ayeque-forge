# Ayeque Forge

Ayeque Forge stores and materializes Git-backed entities for a workspace.
An entity is a directory identified by an ID, kind, schema, Git source,
revision, and optional path inside that source repository.

Forge keeps its catalog and checkouts outside the source workspace. The
workspace therefore stays free of generated Forge files.

## Quick start

Build the CLI:

```sh
cargo build --release
```

Initialize Forge for the current Git workspace:

```sh
ayeque-forge init
```

This creates the workspace's Forge project in XDG data storage. Edit its
`FORGE.toml`, then resolve and materialize the declared entities:

```sh
ayeque-forge lock
ayeque-forge validate
```

Resolve one entity for a script or another program:

```sh
entity_root="$(ayeque-forge path agentlibre.memory)"
```

`path` prints only the absolute path and does not modify Forge state.

## Manifest

The project manifest is `FORGE.toml` in Forge's XDG project directory. It is
the editable catalog; `FORGE.lock` is generated from it and records the exact
commits and Git tree IDs used by the materialized entities.

Example:

```toml
format = 2

[[entity]]
id = "agentlibre.memory"
kind = "memory"
schema = "agentlibre.memory/v1"
git = "https://github.com/example/agentlibre-memory.git"
revision = "acfc5ae"
path = "."
```

`path` defaults to `.` and is useful when several entities are stored in one
repository. Entity IDs are also used as directory names, so they must be safe
single path components.

Run `ayeque-forge lock` after changing the manifest. Forge fetches each pinned
revision, verifies the requested tree, and replaces the affected
materialization together with the lock as one transaction. A failed or
interrupted transaction is recovered before the next mutation.

Do not edit `FORGE.lock` by hand.

## Storage

The default storage root is:

```text
${XDG_DATA_HOME:-$HOME/.local/share}/ayeque-forge/
```

Its relevant contents are:

```text
git/                              managed bare Git repositories
projects/<project-key>/
  project.toml                    workspace identity
  FORGE.toml                      editable entity catalog
  FORGE.lock                      resolved entity identities
  entities/<entity-id>/           materialized entity tree
```

To use another storage root:

```sh
ayeque-forge config storage-root /absolute/path/to/forge
```

The override is global for the current user and must be absolute.

## Commands

```text
ayeque-forge init [PATH]          initialize a workspace project
ayeque-forge validate             verify the manifest, lock, and entities
ayeque-forge lock                 resolve and materialize the manifest
ayeque-forge path <ID>            print one materialized entity path
ayeque-forge paths                print Forge and entity paths
ayeque-forge config storage-root PATH
                                  set the global storage root
```

`init [PATH]` defaults to the current directory. `validate` and `path` are
read-only. `paths` is intended for diagnostics and prints the workspace,
project, storage, cache, lock, and materialized entity locations.

## Library

The `ayeque-forge-core` crate contains the project resolver, lock verifier,
entity resolver, and transactional registration APIs. Consumers should use
the verified entity path returned by the library or CLI; they should not scan
the Forge cache themselves.

The CLI and core share the same manifest, lock, and materialization rules.

## Development

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
scripts/live-smoke.sh target/debug/ayeque-forge
```

Do not add `FORGE.toml`, `FORGE.lock`, generated checkouts, or materialized
entities to this repository. They belong to Forge's XDG storage.

## License

Licensed under the GNU Lesser General Public License, version 3 or later.
See [LICENSE](LICENSE).
