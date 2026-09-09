# Ayeque Forge

Ayeque Forge is the repository, authoring, dependency, versioning, and
distribution layer for typed entities. Entity kinds are extensible and are not
built into the storage model.

The CLI is intentionally small:

```text
ayeque-forge init [PATH]
ayeque-forge validate
ayeque-forge config storage-root PATH
ayeque-forge lock
ayeque-forge path <ID> [--change PATH]
ayeque-forge paths
```

Build and install the CLI into the Cargo bin directory used by `PATH`:

```sh
scripts/install-ayeque-forge.sh
```

It installs the release binary as `ayeque-forge` under
`${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin`. Use `--root PATH`
for another prefix, `--debug` for a debug build, or `--dry-run` to inspect the
installation plan.

`init` initializes the workspace, creates the managed storage layout, and
creates an initial `FORGE.lock` when absent. It does not change an existing
manifest. Use `validate` to explicitly validate `FORGE.toml` and `FORGE.lock`.
The default storage root
is `$XDG_DATA_HOME/ayeque-forge` (or `$HOME/.local/share/ayeque-forge`), with
`git/` and `checkouts/` below it. The workspace itself only needs the
committed `FORGE.toml` and `FORGE.lock`; materialized artifacts do not need to
live in the repository.

After initialization, add the two workspace files to Git:

```sh
git add FORGE.toml FORGE.lock
```

`lock` resolves the manifest's Git revisions, verifies and materializes their
directory trees, then atomically replaces `FORGE.lock`.

If XDG storage is not suitable, set a workspace-relative or absolute root:

```sh
ayeque-forge config storage-root .forge-data
ayeque-forge config storage-root /var/cache/agentlibre/forge
```

This writes the setting to `FORGE.toml`:

```toml
[storage]
root = ".forge-data"
```

Relative roots are resolved from the directory containing `FORGE.toml`.
Changing the root changes the manifest and therefore requires
`ayeque-forge lock` again.

An individual entity can use its own artifact directory:

```sh
ayeque-forge path agent --change /srv/agentlibre/agents/agent
ayeque-forge path agent --change ../artifacts/agent
ayeque-forge lock
```

This adds `artifact` to that entity in `FORGE.toml`. The path is resolved
relative to the workspace when it is not absolute. The managed Git cache still
uses the shared storage root.

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

`paths` prints the workspace, manifest, lock, storage, Git cache, checkout
directories, and the materialized path for every locked entity. It is intended
for agents and diagnostics; `path <ID>` remains the script-friendly
single-path interface.

The entity content does not live in this repository. The root
`FORGE.toml` declares the `docs` and `tasks` entities against local Git
repositories in the storage root (`entities/docs` and `entities/tasks`),
so after `ayeque-forge lock`:

```sh
docs_root="$(ayeque-forge path docs)"
tasks_root="$(ayeque-forge path tasks)"
```
