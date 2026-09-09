# AGL-204: Forge workspace CLI

## Status

Superseded by `ayeque-forge/0002-unified-artifact-catalog.md`.

The accepted contract is XDG-owned Forge state: source workspaces contain no
`FORGE.toml`, `FORGE.lock`, generated checkout, or materialized entity.
`ayeque-forge init` claims `projects/<workspace-basename>` (with a
deterministic hash suffix on collision), and all commands resolve that project
from the current workspace. Entity `path` is optional and defaults to `.`;
per-entity artifact destinations and `path --change` are removed.

Use the current CLI contract:

    ayeque-forge init [PATH]
    ayeque-forge validate
    ayeque-forge config storage-root ABSOLUTE_PATH
    ayeque-forge lock
    ayeque-forge path ID
    ayeque-forge paths
