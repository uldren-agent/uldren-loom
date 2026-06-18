#!/usr/bin/env bash
set -euo pipefail

manifest_path="${1:-target/release-materials/standard/bindings/binding-release-materials.json}"
out_dir="${2:-$(dirname "$manifest_path")}"

mkdir -p "$out_dir"

python3 - "$manifest_path" "$out_dir" <<'PY'
import json
import os
import sys

manifest_path, out_dir = sys.argv[1:3]

with open(manifest_path, "r", encoding="utf-8") as f:
    manifest = json.load(f)

allowed_registries = {
    "cocoapods",
    "github-release",
    "maven-central",
    "npm",
    "pypi",
    "swiftpm-git-tag",
}
allowed_credentials = {
    "github-actions-oidc-or-release-app-token",
    "registry-oidc-or-scoped-publishing-secret",
    "scoped-trunk-token-or-signed-source-tag",
    "signed-git-tag-and-github-release-oidc",
    "trusted-publisher-oidc-required",
}

errors = []

if manifest.get("schema") != "loom.binding-release-materials.v1":
    errors.append("manifest schema must be loom.binding-release-materials.v1")

policy = manifest.get("publication_policy", {})
if policy.get("status") != "unpublished":
    errors.append("publication policy status must remain unpublished")
if policy.get("registry_publication_is_target") is not True:
    errors.append("registry publication must remain target in the dry-run gate")
if policy.get("signed_artifact_status") != "unsigned":
    errors.append("binding artifacts must remain unsigned in the dry-run gate")
if policy.get("attestation_status") != "target":
    errors.append("registry attestation must remain target in the dry-run gate")
if policy.get("install_validation_required") is not True:
    errors.append("install validation policy must be required")

bindings = manifest.get("bindings", [])
if not bindings:
    errors.append("manifest must contain bindings")

dry_run_bindings = []
for binding in bindings:
    name = binding.get("name", "")
    package_name = binding.get("package_name", "")
    routes = binding.get("publication_routes", [])
    install_validation = binding.get("install_validation", "")

    if not name:
        errors.append("binding entry must have a name")
    if not package_name:
        errors.append(f"{name} must have a package_name")
    if not routes:
        errors.append(f"{name} must declare publication routes")
    if not install_validation:
        errors.append(f"{name} must declare install validation")

    route_plan = []
    for route in routes:
        registry = route.get("registry", "")
        artifact = route.get("artifact", "")
        credential_policy = route.get("credential_policy", "")
        if registry not in allowed_registries:
            errors.append(f"{name} has unknown registry {registry}")
        if not artifact:
            errors.append(f"{name} route for {registry} must declare artifact")
        if credential_policy not in allowed_credentials:
            errors.append(f"{name} route for {registry} has unknown credential policy {credential_policy}")
        route_plan.append({
            "registry": registry,
            "artifact": artifact,
            "credential_policy": credential_policy,
            "publish_action": "not-run",
            "publish_reason": "dry-run gate does not publish or read registry credentials",
        })

    dry_run_bindings.append({
        "name": name,
        "package_name": package_name,
        "package_kind": binding.get("package_kind", ""),
        "publication_routes": route_plan,
        "install_validation": install_validation,
        "install_validation_action": "policy-checked-not-run-against-registry",
    })

if errors:
    for error in errors:
        print(f"binding publication dry-run failed: {error}", file=sys.stderr)
    sys.exit(1)

github_ref = os.environ.get("GITHUB_REF", "")
github_event = os.environ.get("GITHUB_EVENT_NAME", "")
github_ref_protected = os.environ.get("GITHUB_REF_PROTECTED", "")

report = {
    "schema": "loom.binding-publication-dry-run.v1",
    "channel": manifest.get("channel", ""),
    "source_revision": manifest.get("source_revision", ""),
    "materials_manifest_file": os.path.basename(manifest_path),
    "live_publish": False,
    "credential_access": "not-read",
    "release_environment": os.environ.get("BINDING_RELEASE_ENVIRONMENT", "binding-release"),
    "github_ref": github_ref,
    "github_event": github_event,
    "github_ref_protected": github_ref_protected,
    "release_ref_policy": {
        "manual_dispatch_allowed": github_event == "workflow_dispatch",
        "tag_release_allowed": github_ref.startswith("refs/tags/v"),
        "pull_request_allowed": False,
    },
    "publication_policy": policy,
    "bindings": dry_run_bindings,
}

report_path = os.path.join(out_dir, "binding-publication-dry-run.json")
with open(report_path, "w", encoding="utf-8") as f:
    json.dump(report, f, indent=2, sort_keys=True)
    f.write("\n")

print(f"binding publication dry-run written to {report_path}")
PY
