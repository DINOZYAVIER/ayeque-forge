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
ayeque-forge path <ID>
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

`init` claims an XDG project catalog for the workspace and creates an initial
`FORGE.toml` and `FORGE.lock` when absent. It never writes Forge files into
the source workspace. The default storage root is
`$XDG_DATA_HOME/ayeque-forge` (or `$HOME/.local/share/ayeque-forge`), with
`git/` and `projects/` below it.

`lock` resolves the manifest's Git revisions, verifies and materializes their
directory trees, then atomically replaces `FORGE.lock`.

If XDG storage is not suitable, set an absolute global root:

```sh
ayeque-forge config storage-root /var/cache/agentlibre/forge
```

`path` resolves the current workspace's XDG project and prints the absolute
materialized path for one locked entity. Its stdout contains only that path:

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

`paths` prints the workspace, XDG project, manifest, lock, storage, Git cache,
projects, and materialized path for every locked entity. It is intended
for agents and diagnostics; `path <ID>` remains the script-friendly
single-path interface.

The source workspace contains no Forge manifest, lock, generated checkout, or
materialized entity. Entity declarations are edited in the XDG project
manifest under `projects/<project-key>/FORGE.toml`.
