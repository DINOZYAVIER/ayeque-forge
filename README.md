# Ayeque Forge

Ayeque Forge is the repository, authoring, dependency, versioning, and
distribution layer for typed entities. Entity kinds are extensible and are not
built into the storage model.

The alpha CLI intentionally has three commands:

```text
ayeque-forge init [PATH]
ayeque-forge lock
ayeque-forge path <ID>
```

`init` creates an empty `FORGE.toml` in an existing directory. `lock` resolves
the manifest's Git revisions, verifies and materializes their directory trees,
then atomically replaces `FORGE.lock`.

`path` finds the nearest `FORGE.toml`, reads the adjacent `FORGE.lock`, and
prints the absolute materialized path for one locked entity. Its stdout contains
only that path so a consumer can use it directly:

```sh
function_root="$(ayeque-forge path coder)" &&
agent_root="$(ayeque-forge path coder-agent)" &&
model_root="$(ayeque-forge path coder-model)" &&
agl --function "$function_root" \
  --entity-root "$agent_root" \
  --entity-root "$model_root"
```

`path` does not fetch or modify data. It fails when the manifest has changed
since `lock`, the id is absent, or the materialized checkout is unavailable.
Consumers receive explicit paths; they do not scan the Forge cache and do not
need to understand `FORGE.toml` or `FORGE.lock`.
