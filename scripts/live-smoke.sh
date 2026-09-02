#!/usr/bin/env bash
set -euo pipefail

forge_bin="${1:-target/release/ayeque-forge}"
forge_bin="$(readlink -f "$forge_bin")"
smoke_root="$(mktemp -d)"
trap 'rm -rf "$smoke_root"' EXIT

init_dir="$smoke_root/init"
mkdir -p "$init_dir"
"$forge_bin" init "$init_dir"
"$forge_bin" init "$init_dir"
test "$(cat "$init_dir/FORGE.toml")" = "format = 1"

source_repo="$smoke_root/source"
workspace="$smoke_root/workspace"
mkdir -p "$source_repo/one" "$source_repo/two" "$workspace/nested"
printf 'one\n' > "$source_repo/one/value.txt"
printf 'two\n' > "$source_repo/two/value.txt"
git -C "$source_repo" init --initial-branch=main
git -C "$source_repo" config user.name "Forge Smoke"
git -C "$source_repo" config user.email "forge-smoke@example.invalid"
git -C "$source_repo" add .
git -C "$source_repo" commit -m fixture
cat > "$workspace/FORGE.toml" <<EOF
format = 1

[[entity]]
id = "one"
kind = "smoke"
schema = "1"
git = "$source_repo"
revision = "main"
path = "one"

[[entity]]
id = "two"
kind = "smoke"
schema = "1"
git = "$source_repo"
revision = "main"
path = "two"
EOF
(
    cd "$workspace/nested"
    XDG_DATA_HOME="$smoke_root/data" "$forge_bin" lock
)
test "$(grep -c '^commit = ' "$workspace/FORGE.lock")" = 2
old_lock="$(sha256sum "$workspace/FORGE.lock" | cut -d ' ' -f1)"
sed -i 's/path = "two"/path = "missing"/' "$workspace/FORGE.toml"
if (
    cd "$workspace"
    XDG_DATA_HOME="$smoke_root/data" "$forge_bin" lock
); then
    echo "lock unexpectedly accepted a missing entity path" >&2
    exit 1
fi
test "$(sha256sum "$workspace/FORGE.lock" | cut -d ' ' -f1)" = "$old_lock"

lfs_repo="$smoke_root/lfs-source"
lfs_workspace="$smoke_root/lfs-workspace"
mkdir -p "$lfs_repo/entity" "$lfs_workspace"
git -C "$lfs_repo" init --initial-branch=main
git -C "$lfs_repo" config user.name "Forge Smoke"
git -C "$lfs_repo" config user.email "forge-smoke@example.invalid"
git -C "$lfs_repo" lfs install --local
git -C "$lfs_repo" lfs track '*.bin'
printf 'materialized LFS payload\n' > "$lfs_repo/entity/payload.bin"
git -C "$lfs_repo" add .
git -C "$lfs_repo" commit -m lfs-fixture
cat > "$lfs_workspace/FORGE.toml" <<EOF
format = 1

[[entity]]
id = "lfs"
kind = "smoke"
schema = "1"
git = "$lfs_repo"
revision = "main"
path = "entity"
EOF
lfs_output="$(
    cd "$lfs_workspace"
    XDG_DATA_HOME="$smoke_root/lfs-data" "$forge_bin" lock
)"
materialized="$(printf '%s\n' "$lfs_output" | cut -f2)"
cmp "$lfs_repo/entity/payload.bin" "$materialized/payload.bin"

rm -rf "$lfs_repo/.git/lfs/objects"
missing_workspace="$smoke_root/missing-lfs-workspace"
mkdir -p "$missing_workspace"
cp "$lfs_workspace/FORGE.toml" "$missing_workspace/FORGE.toml"
if (
    cd "$missing_workspace"
    XDG_DATA_HOME="$smoke_root/missing-lfs-data" "$forge_bin" lock
); then
    echo "lock unexpectedly accepted a missing LFS object" >&2
    exit 1
fi
test ! -e "$missing_workspace/FORGE.lock"

echo "ayeque-forge release live smoke passed"
