# Ayeque Forge docs

Documentation for the Ayeque Forge repository, authoring, dependency,
versioning, and distribution layer for typed entities.

This folder is an entity in the root `FORGE.toml`. Consumers resolve its
materialized checkout without scanning this repository:

```sh
docs_root="$(ayeque-forge path docs)"
```

## Contents

- Project overview and CLI reference: the repository `README.md`.
- Task tracking: the `tasks` entity.
