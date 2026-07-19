#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sample_manifest="$repo_root/examples/third-party-opacity-effector/Cargo.toml"
temporary_root="$(mktemp -d)"
trap 'rm -rf "$temporary_root"' EXIT

# Keep the host and independently compiled plugin in permanently distinct
# target trees. Callers/CI can opt into reusable build caches by setting
# CARGO_TARGET_DIR; an ordinary local run remains self-cleaning.
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    runtime_target_root="$CARGO_TARGET_DIR/runtime-plugin-e2e"
else
    runtime_target_root="$temporary_root/targets"
fi
host_target="$runtime_target_root/host"
sample_target="$runtime_target_root/sample"

# Establish the required ordering: both the app and the exact executable probe
# exist before the third-party plugin is compiled. The sample uses a separate
# target directory, so it cannot rebuild either host executable afterwards.
cargo build --manifest-path "$repo_root/Cargo.toml" -p app \
    --target-dir "$host_target" --locked
cargo build --manifest-path "$repo_root/Cargo.toml" -p library \
    --bin runtime_plugin_probe --target-dir "$host_target" --locked
host_probe="$host_target/debug/runtime_plugin_probe"
host_fingerprint="$(cksum "$host_probe")"

dependency_tree="$(cargo tree --manifest-path "$sample_manifest" --locked --prefix none)"
if printf '%s\n' "$dependency_tree" | grep -Eq '^(app|library) v'; then
    printf '%s\n' "standalone plugin unexpectedly depends on host implementation" >&2
    exit 1
fi

cargo build --manifest-path "$sample_manifest" --target-dir "$sample_target" --locked

bundle="$temporary_root/third-party-opacity"
mkdir -p "$bundle"
cp "$repo_root/examples/third-party-opacity-effector/ruvie-plugin.toml" "$bundle/"

case "$(uname -s)" in
    Darwin) library_name="libthird_party_opacity_effector.dylib" ;;
    Linux) library_name="libthird_party_opacity_effector.so" ;;
    *) printf '%s\n' "Use the documented PowerShell-equivalent steps on Windows." >&2; exit 1 ;;
esac
cp "$sample_target/debug/$library_name" "$bundle/$library_name"

"$host_probe" "$bundle"

# The ignored integration test is deliberately non-vacuous: this script must
# supply the independently built bundle, and the test concurrently rescans it
# while counting descriptor callbacks inside the loaded library.
RUVIE_TEST_PLUGIN_BUNDLE="$bundle" cargo test \
    --manifest-path "$repo_root/Cargo.toml" -p library \
    --test runtime_plugin_tests --target-dir "$host_target" --locked \
    standalone_runtime_effector_loads_describes_builds_and_invokes -- \
    --ignored --exact

host_after_plugin="$(cksum "$host_probe")"
if [[ "$host_after_plugin" != "$host_fingerprint" ]]; then
    printf '%s\n' "host probe changed after third-party plugin build" >&2
    exit 1
fi
