# Ayeque Forge tasks

Working tasks for Ayeque Forge.

This folder is an entity in the root `FORGE.toml`. Consumers resolve its
materialized checkout without scanning this repository:

```sh
tasks_root="$(ayeque-forge path tasks)"
```

## Conventions

- One file per task, named `NNNN-short-name.md`.
- Each task records its outcome, steps, and verification.
- Completed tasks stay in place so history remains auditable.
