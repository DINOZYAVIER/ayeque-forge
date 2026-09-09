#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
    printf 'usage: %s ENTITY ARTIFACT_PATH [FORGE_BIN]\n' "$0" >&2
    exit 2
fi

entity="$1"
artifact_path="$2"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
forge_bin="${3:-${AYEQUE_FORGE_BIN:-$repo_root/target/release/ayeque-forge}}"

if [[ ! -x "$forge_bin" ]]; then
    printf 'ayeque-forge binary is not executable: %s\n' "$forge_bin" >&2
    printf 'build it with scripts/build-ayeque-forge.sh or set AYEQUE_FORGE_BIN\n' >&2
    exit 1
fi

exec "$forge_bin" path "$entity" --change "$artifact_path"
