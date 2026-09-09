#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"

profile="${1:-release}"
case "$profile" in
    release)
        cargo_args=(build --locked --release)
        binary="$repo_root/target/release/ayeque-forge"
        ;;
    debug)
        cargo_args=(build --locked)
        binary="$repo_root/target/debug/ayeque-forge"
        ;;
    *)
        printf 'usage: %s [release|debug]\n' "$0" >&2
        exit 2
        ;;
esac

cd "$repo_root"
cargo "${cargo_args[@]}"
printf '%s\n' "$binary"
