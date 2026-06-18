#!/usr/bin/env bash
set -euo pipefail

channel="${1:-standard}"
out_root="${2:-target/release-materials}"

case "$channel" in
    standard|fips) ;;
    *)
        echo "usage: $0 <standard|fips> [out-dir]" >&2
        exit 2
        ;;
esac

out_dir="${out_root%/}/${channel}/bindings"
mkdir -p "$out_dir"

source_revision="${LOOM_SOURCE_REVISION:-}"
if [ -z "$source_revision" ] && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    source_revision="$(git rev-parse HEAD)"
fi

python3 - "$channel" "$out_dir" "$source_revision" <<'PY'
import glob
import hashlib
import json
import os
import sys

channel, out_dir, source_revision = sys.argv[1:4]

def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def file_ref(path):
    if not os.path.isfile(path):
        return {
            "path": path,
            "present": False,
            "sha256": "",
        }
    return {
        "path": path,
        "present": True,
        "sha256": sha256(path),
    }

def workspace_version():
    in_workspace_package = False
    with open("Cargo.toml", "r", encoding="utf-8") as f:
        for raw_line in f:
            line = raw_line.strip()
            if line == "[workspace.package]":
                in_workspace_package = True
                continue
            if line.startswith("[") and line != "[workspace.package]":
                in_workspace_package = False
            if in_workspace_package and line.startswith("version"):
                return line.split("=", 1)[1].strip().strip('"')
    return ""

def existing(patterns):
    out = []
    for pattern in patterns:
        out.extend(glob.glob(pattern))
    return sorted({
        path
        for path in out
        if os.path.isfile(path) and not path.endswith(".d")
    })

def artifacts(patterns):
    return [
        {
            "path": path,
            "sha256": sha256(path),
        }
        for path in existing(patterns)
    ]

def binding(
    name,
    package_name,
    package_kind,
    build_recipe,
    runtime_profile,
    fips_native_claim,
    publication_routes,
    install_validation,
    artifact_patterns,
    notes=None,
):
    return {
        "name": name,
        "package_name": package_name,
        "package_kind": package_kind,
        "build_recipe": build_recipe,
        "runtime_profile_surface": runtime_profile,
        "fips_native_claim_allowed": bool(fips_native_claim),
        "fips_feature_required": channel == "fips" and bool(fips_native_claim),
        "publication_routes": publication_routes,
        "install_validation": install_validation,
        "artifact_profile_verified_by_this_script": False,
        "artifacts": artifacts(artifact_patterns),
        "notes": notes or [],
    }

is_fips = channel == "fips"
version = workspace_version()
bindings = [
    binding(
        "c-abi",
        "libuldren_loom",
        "native-library",
        "just ffi-fips" if is_fips else "just ffi",
        "loom_runtime_profile",
        is_fips,
        [
            {
                "registry": "github-release",
                "artifact": "native-library",
                "credential_policy": "github-actions-oidc-or-release-app-token",
            },
        ],
        "download release asset, verify checksum and signature, load the native library, call loom_version, and call loom_runtime_profile",
        [
            "target/release/libuldren_loom.a",
            "target/release/libuldren_loom.dylib",
            "target/release/libuldren_loom.so",
            "target/release/uldren_loom.dll",
        ],
    ),
    binding(
        "node",
        "@uldrenai/loom",
        "npm-native-addon",
        "cd bindings/node && pnpm run build",
        "runtimeProfile()",
        is_fips,
        [
            {
                "registry": "npm",
                "artifact": "@uldrenai/loom",
                "credential_policy": "trusted-publisher-oidc-required",
            },
        ],
        "install the packed npm artifact in a clean project, require the package, call version, call runtimeProfile, and run test.mjs",
        ["bindings/node/*.node"],
        ["Build the native addon with Cargo feature `fips` before making a FIPS package claim."] if is_fips else [],
    ),
    binding(
        "python",
        "uldrenai-loom",
        "python-native-extension",
        "cd bindings/python && maturin build --release",
        "runtime_profile()",
        is_fips,
        [
            {
                "registry": "pypi",
                "artifact": "uldrenai-loom",
                "credential_policy": "trusted-publisher-oidc-required",
            },
        ],
        "install the wheel in a clean virtual environment, import uldrenai_loom, call version, and call runtime_profile",
        [
            "bindings/python/target/wheels/*.whl",
            "bindings/python/python/uldrenai_loom/_native*",
        ],
        ["Build the extension with Cargo feature `fips` before making a FIPS package claim."] if is_fips else [],
    ),
    binding(
        "cpp",
        "loom-cpp",
        "header-plus-native-library",
        "just cpp",
        "loom::runtime_profile()",
        is_fips,
        [
            {
                "registry": "github-release",
                "artifact": "header-plus-native-library",
                "credential_policy": "github-actions-oidc-or-release-app-token",
            },
        ],
        "download release headers and native library, verify checksum and signature, compile the C++ example, and call runtime_profile",
        ["bindings/cpp/include/**/*.hpp", "include/loom.h", "target/release/libuldren_loom.*"],
    ),
    binding(
        "ios",
        "UldrenLoom",
        "swiftpm-plus-native-library",
        "just ios",
        "Loom.runtimeProfile()",
        is_fips,
        [
            {
                "registry": "swiftpm-git-tag",
                "artifact": "UldrenLoom",
                "credential_policy": "signed-git-tag-and-github-release-oidc",
            },
            {
                "registry": "github-release",
                "artifact": "xcframework",
                "credential_policy": "github-actions-oidc-or-release-app-token",
            },
        ],
        "resolve the Swift package by release tag or XCFramework asset, verify checksum and signature, run swift test, and call Loom.runtimeProfile",
        ["bindings/ios/Sources/**/*.swift", "bindings/ios/Sources/CUldrenLoom/include/loom.h"],
    ),
    binding(
        "jvm",
        "ai.uldren:loom",
        "jvm-plus-native-library",
        "just jvm",
        "Loom.runtimeProfile()",
        is_fips,
        [
            {
                "registry": "maven-central",
                "artifact": "ai.uldren:loom",
                "credential_policy": "registry-oidc-or-scoped-publishing-secret",
            },
        ],
        "resolve the Maven artifact in a clean Gradle project, verify native classifier selection, call Loom.version, and call Loom.runtimeProfile",
        ["bindings/jvm/build/libs/*.jar", "target/release/libuldren_loom.*"],
    ),
    binding(
        "android",
        "ai.uldren:loom-android",
        "android-aar-plus-native-library",
        "just android",
        "Loom.runtimeProfile()",
        is_fips,
        [
            {
                "registry": "maven-central",
                "artifact": "ai.uldren:loom-android",
                "credential_policy": "registry-oidc-or-scoped-publishing-secret",
            },
        ],
        "resolve the Android artifact in a clean Gradle project, run the JVM runtime smoke test, and run a connected device or emulator smoke test for native ABI loading",
        ["bindings/android/build/outputs/**/*.aar", "target/*/release/libuldren_loom.a"],
    ),
    binding(
        "react-native",
        "@uldrenai/loom-react-native",
        "npm-react-native-plus-native-library",
        "just react-native-android",
        "runtimeProfile()",
        is_fips,
        [
            {
                "registry": "npm",
                "artifact": "@uldrenai/loom-react-native",
                "credential_policy": "trusted-publisher-oidc-required",
            },
            {
                "registry": "cocoapods",
                "artifact": "UldrenLoom",
                "credential_policy": "scoped-trunk-token-or-signed-source-tag",
            },
        ],
        "install the npm package into a clean React Native app, resolve the Android native package, resolve the podspec where supported, and run the connected host fixture",
        ["bindings/react-native/**/*.aar", "target/*/release/libuldren_loom.a"],
    ),
    binding(
        "wasm",
        "@uldrenai/loom-wasm",
        "wasm-browser-package",
        "just wasm",
        "runtime_profile()",
        False,
        [
            {
                "registry": "npm",
                "artifact": "@uldrenai/loom-wasm",
                "credential_policy": "trusted-publisher-oidc-required",
            },
        ],
        "install the npm package in a clean browser-worker fixture, initialize wasm-bindgen output, call version, call runtime_profile, and run the OPFS smoke test",
        ["bindings/wasm/pkg/**/*"],
        ["WASM exposes compatibility profile reporting but does not make a native FIPS certification claim."],
    ),
]

manifest = {
    "schema": "loom.binding-release-materials.v1",
    "channel": channel,
    "source_revision": source_revision,
    "checksums_file": "binding-checksums.sha256",
    "signing_manifest_file": "binding-signing-manifest.json",
    "compatibility_metadata": {
        "schema": "loom.binding-compatibility.v1",
        "workspace_version": version,
        "package_version_source": "Cargo.toml [workspace.package].version",
        "core_abi": {
            "header": file_ref("include/loom.h"),
            "idl": file_ref("idl/loom.idl"),
        },
        "binding_packages_must_match_workspace_version": True,
    },
    "publication_policy": {
        "status": "unpublished",
        "registry_publication_is_target": True,
        "signed_artifact_status": "unsigned",
        "attestation_status": "target",
        "registry_credentials_required": True,
        "release_channel_required": True,
        "credential_policy": "prefer registry OIDC or trusted publishing; use scoped release secrets only when a registry lacks OIDC",
        "attestation_policy": "release attestations must bind source_revision, binding-checksums.sha256, binding-release-materials.json, binding-signing-manifest.json, compatibility metadata, and the conformance report",
        "install_validation_required": True,
    },
    "policy": {
        "fips_native_claim_requires_fips_channel": True,
        "wasm_fips_certification_claim": False,
        "artifact_profile_is_not_inferred_from_path": True,
        "package_name_policy": "current package names are recorded; alternate FIPS package names are a release-channel decision",
    },
    "bindings": bindings,
}

manifest_path = os.path.join(out_dir, "binding-release-materials.json")
with open(manifest_path, "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2, sort_keys=True)
    f.write("\n")

checksums = {}
for b in bindings:
    for artifact in b["artifacts"]:
        checksums[artifact["path"]] = artifact["sha256"]
checksums[manifest_path] = sha256(manifest_path)

checksums_path = os.path.join(out_dir, "binding-checksums.sha256")
with open(checksums_path, "w", encoding="utf-8") as f:
    for path, digest in sorted(checksums.items()):
        f.write(f"{digest}  {path}\n")

signing_manifest = {
    "schema": "loom.binding-signing-manifest.v1",
    "channel": channel,
    "source_revision": source_revision,
    "status": "unsigned",
    "signing_key_id": os.environ.get("LOOM_SIGNING_KEY_ID", ""),
    "checksums_file": "binding-checksums.sha256",
    "materials_manifest_file": "binding-release-materials.json",
    "artifacts": [
        {
            "path": path,
            "sha256": digest,
        }
        for path, digest in sorted(checksums.items())
    ],
}
signing_manifest_path = os.path.join(out_dir, "binding-signing-manifest.json")
with open(signing_manifest_path, "w", encoding="utf-8") as f:
    json.dump(signing_manifest, f, indent=2, sort_keys=True)
    f.write("\n")

print(f"binding release materials written to {out_dir}")
PY
