# 0001: Self-host Forge

## Outcome

The Ayeque Forge repository is a Forge workspace. `FORGE.toml` declares the
`docs` and `tasks` entities against this repository, and `FORGE.lock` pins
their resolved commits and trees.

## Steps

1. Add `docs/` and `tasks/` to the repository.
2. Add a root `FORGE.toml` declaring both entities with `revision = "main"`.
3. Commit the manifest and folders, then run `ayeque-forge lock`.
4. Verify `ayeque-forge path docs` and `ayeque-forge path tasks`.

## Verification

- `lock` prints one line per entity with its materialized path.
- `path docs` and `path tasks` each print an absolute directory whose content
  matches the committed `docs/` and `tasks/` folders.
