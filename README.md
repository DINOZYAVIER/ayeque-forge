# Ayeque Forge

Ayeque Forge is the repository, authoring, dependency, versioning, and
distribution layer for typed entities. Entity kinds are extensible and are not
built into the storage model.

The alpha CLI intentionally has two commands:

```text
ayeque-forge init [PATH]
ayeque-forge lock
```

`init` creates an empty `FORGE.toml` in an existing directory. `lock` resolves
the manifest's Git revisions, verifies and materializes their directory trees,
then atomically replaces `FORGE.lock`.

