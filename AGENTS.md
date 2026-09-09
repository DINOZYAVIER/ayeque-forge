# Agent instructions

## Forge workspace workflow

Forge owns manifests, locks, project identity, and materialized entities in
XDG storage. Source repositories contain no Forge state.

    ayeque-forge init
    ayeque-forge validate
    ayeque-forge lock
    ayeque-forge paths
    entity_root="$(ayeque-forge path agent)"

The default data root is `$XDG_DATA_HOME/ayeque-forge` or
`$HOME/.local/share/ayeque-forge`, containing `git/` and `projects/`.
`config storage-root PATH` is the one global override and requires an
absolute path.

Entity `path` is optional and defaults to `.`; use it only for a source
subdirectory in a monorepo. Every materialized entity is
`projects/<project-key>/entities/<entity-id>`.

## Development commands

    scripts/build-ayeque-forge.sh
    cargo fmt --check
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
    scripts/live-smoke.sh target/debug/ayeque-forge

Do not create or commit `FORGE.toml`, `FORGE.lock`, generated checkouts, or
materialized entities in this repository.
