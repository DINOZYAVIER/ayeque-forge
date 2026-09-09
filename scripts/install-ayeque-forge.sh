#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"

usage() {
    cat <<'EOF'
Usage: scripts/install-ayeque-forge.sh [options]

Builds and installs `ayeque-forge` into one Cargo binary prefix.
The installed binary replaces the existing `ayeque-forge` by default.

Options:
  --root PATH       installation prefix (default: Cargo install root)
  --debug           build and install the debug binary
  --no-force        refuse to replace an existing binary
  --dry-run         print the planned commands without executing them
  -h, --help        show this help
EOF
}

run() {
    printf '+'
    printf ' %q' "$@"
    printf '\n'
    if [[ "$dry_run" -eq 0 ]]; then
        "$@"
    fi
}

root="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-${HOME:?HOME is required}/.cargo}}"
profile="release"
force=1
dry_run=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --root)
            root="${2:?missing value for --root}"
            shift 2
            ;;
        --debug)
            profile="debug"
            shift
            ;;
        --no-force)
            force=0
            shift
            ;;
        --dry-run)
            dry_run=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [[ "$root" != /* ]]; then
    root="$PWD/$root"
fi
root="$(realpath -m -s -- "$root")"
[[ "$(realpath -m -- "$root")" == "$root" ]] || {
    echo "installation prefix traverses a symlink: $root" >&2
    exit 1
}

if [[ "$profile" == "release" ]]; then
    binary="$repo_root/target/release/ayeque-forge"
    build_args=(build --locked --release)
else
    binary="$repo_root/target/debug/ayeque-forge"
    build_args=(build --locked)
fi

target="$root/bin/ayeque-forge"

if [[ "$dry_run" -eq 0 ]]; then
    command -v cargo >/dev/null 2>&1 || {
        echo "missing required tool: cargo" >&2
        exit 1
    }
    command -v install >/dev/null 2>&1 || {
        echo "missing required tool: install" >&2
        exit 1
    }
fi

if [[ "$force" -eq 0 && -e "$target" ]]; then
    echo "refusing to replace existing binary: $target" >&2
    exit 1
fi

run cargo "${build_args[@]}"
run install -d -m 0755 "$root/bin"
run install -m 0755 "$binary" "$target"

if [[ "$dry_run" -eq 0 ]]; then
    "$target" --help >/dev/null
    echo "installed ayeque-forge: $target"
else
    echo "dry-run complete; target would be: $target"
fi
