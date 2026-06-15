#!/usr/bin/env bash
# Propagate the workspace version (the single source of truth in the root Cargo.toml's
# [workspace.package]) into the language-binding manifests, which are not managed by release-plz.
# Run via `just sync-versions`; also part of `just all`.
#
# Lockfiles are intentionally not touched here; they regenerate on the next install/build.
set -euo pipefail
cd "$(dirname "$0")/.."

version="$(perl -ne 'if (/^version = "([^"]+)"/) { print $1; exit }' Cargo.toml)"
if [ -z "${version:-}" ]; then
    echo "error: could not read [workspace.package] version from Cargo.toml" >&2
    exit 1
fi
echo "workspace version: $version"

# Replace the first top-level `version = "..."` line in a TOML or Gradle Kotlin file.
set_manifest_version() {
    VER="$version" perl -0pi -e 's/^version = "[^"]*"/version = "$ENV{VER}"/m' "$1"
    echo "  updated $1"
}

# Replace the first `"version": "..."` field (the package version) in a JSON file.
set_json_version() {
    VER="$version" perl -0pi -e 's/"version":\s*"[^"]*"/"version": "$ENV{VER}"/' "$1"
    echo "  updated $1"
}

set_manifest_version bindings/node/Cargo.toml
set_manifest_version bindings/wasm/Cargo.toml
set_manifest_version bindings/jvm/build.gradle.kts
set_manifest_version bindings/kotlin/build.gradle.kts
set_json_version bindings/node/package.json
set_json_version bindings/react-native/package.json

echo "binding versions synced to $version"
