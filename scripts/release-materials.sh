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

out_dir="${out_root%/}/${channel}"
mkdir -p "$out_dir"

json_string() {
    printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g; s/	/\\t/g'
}

hash_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

source_revision="${LOOM_SOURCE_REVISION:-}"
if [ -z "$source_revision" ] && git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    source_revision="$(git rev-parse HEAD)"
fi

binary_path="${LOOM_BINARY_PATH:-target/release/loom}"
binary_present=false
binary_sha256=""
binary_artifact_file=""
if [ -f "$binary_path" ]; then
    binary_present=true
    binary_artifact_file="loom"
    cp "$binary_path" "$out_dir/$binary_artifact_file"
    binary_sha256="$(hash_file "$out_dir/$binary_artifact_file")"
fi

cargo metadata --format-version 1 >"$out_dir/cargo-metadata.json"
cargo tree --workspace --edges normal,build >"$out_dir/cargo-tree.txt"
rustc -Vv >"$out_dir/rustc-version.txt"
cargo -Vv >"$out_dir/cargo-version.txt"

python3 - "$out_dir/cargo-metadata.json" "$out_dir/sbom.spdx.json" "$channel" "$source_revision" <<'PY'
import datetime
import json
import re
import sys

metadata_path, sbom_path, channel, source_revision = sys.argv[1:5]
with open(metadata_path, "r", encoding="utf-8") as f:
    metadata = json.load(f)

def spdx_id(name, version):
    raw = f"{name}-{version}"
    clean = re.sub(r"[^A-Za-z0-9.-]", "-", raw)
    return f"SPDXRef-Package-{clean}"

packages = []
for pkg in sorted(metadata.get("packages", []), key=lambda p: (p.get("name", ""), p.get("version", ""))):
    source = pkg.get("source")
    external_refs = []
    if source and "crates.io-index" in source:
        external_refs.append({
            "referenceCategory": "PACKAGE-MANAGER",
            "referenceType": "purl",
            "referenceLocator": f"pkg:cargo/{pkg.get('name')}@{pkg.get('version')}",
        })
    packages.append({
        "name": pkg.get("name", ""),
        "SPDXID": spdx_id(pkg.get("name", ""), pkg.get("version", "")),
        "versionInfo": pkg.get("version", ""),
        "downloadLocation": source or "NOASSERTION",
        "filesAnalyzed": False,
        "licenseConcluded": "NOASSERTION",
        "licenseDeclared": pkg.get("license") or "NOASSERTION",
        "copyrightText": "NOASSERTION",
        "externalRefs": external_refs,
    })

created = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
namespace_revision = source_revision or "unknown"
sbom = {
    "spdxVersion": "SPDX-2.3",
    "dataLicense": "CC0-1.0",
    "SPDXID": "SPDXRef-DOCUMENT",
    "name": f"uldren-loom-{channel}",
    "documentNamespace": f"https://uldren.ai/loom/spdx/{channel}/{namespace_revision}",
    "creationInfo": {
        "created": created,
        "creators": ["Tool: scripts/release-materials.sh"],
    },
    "packages": packages,
}

with open(sbom_path, "w", encoding="utf-8") as f:
    json.dump(sbom, f, indent=2, sort_keys=True)
    f.write("\n")
PY

write_checksums() {
    checksum_path="$out_dir/checksums.sha256"
    : >"$checksum_path"
    for file in \
        cargo-metadata.json \
        cargo-tree.txt \
        rustc-version.txt \
        cargo-version.txt \
        sbom.spdx.json \
        release-materials.json
    do
        if [ -f "$out_dir/$file" ]; then
            printf '%s  %s\n' "$(hash_file "$out_dir/$file")" "$file" >>"$checksum_path"
        fi
    done
    if [ "$binary_present" = true ]; then
        printf '%s  %s\n' "$binary_sha256" "$binary_artifact_file" >>"$checksum_path"
    fi
}

{
    printf '{\n'
    printf '  "channel": "%s",\n' "$(json_string "$channel")"
    printf '  "source_revision": "%s",\n' "$(json_string "$source_revision")"
    printf '  "target": "%s",\n' "$(json_string "${CARGO_BUILD_TARGET:-}")"
    printf '  "profile": "release",\n'
    printf '  "binary_path": "%s",\n' "$(json_string "$binary_path")"
    printf '  "binary_artifact_file": "%s",\n' "$(json_string "$binary_artifact_file")"
    printf '  "binary_present": %s,\n' "$binary_present"
    printf '  "binary_sha256": "%s",\n' "$(json_string "$binary_sha256")"
    printf '  "rustc_version_file": "rustc-version.txt",\n'
    printf '  "cargo_version_file": "cargo-version.txt",\n'
    printf '  "cargo_metadata_file": "cargo-metadata.json",\n'
    printf '  "cargo_tree_file": "cargo-tree.txt",\n'
    printf '  "sbom_file": "sbom.spdx.json",\n'
    printf '  "checksums_file": "checksums.sha256",\n'
    printf '  "signing_manifest_file": "signing-manifest.json"\n'
    printf '}\n'
} >"$out_dir/release-materials.json"

write_checksums

{
    printf '{\n'
    printf '  "channel": "%s",\n' "$(json_string "$channel")"
    printf '  "source_revision": "%s",\n' "$(json_string "$source_revision")"
    printf '  "status": "unsigned",\n'
    printf '  "signing_key_id": "%s",\n' "$(json_string "${LOOM_SIGNING_KEY_ID:-}")"
    printf '  "checksums_file": "checksums.sha256",\n'
    printf '  "artifacts": [\n'
    first=true
    while IFS= read -r line; do
        checksum="${line%%  *}"
        artifact="${line#*  }"
        if [ "$first" = true ]; then
            first=false
        else
            printf ',\n'
        fi
        printf '    {"path": "%s", "sha256": "%s"}' "$(json_string "$artifact")" "$(json_string "$checksum")"
    done <"$out_dir/checksums.sha256"
    printf '\n  ]\n'
    printf '}\n'
} >"$out_dir/signing-manifest.json"

echo "release materials written to $out_dir"
