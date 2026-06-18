#!/usr/bin/env python3
import argparse
import gzip
import hashlib
import json
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
import zipfile
from pathlib import Path
from subprocess import PIPE


def parse_args():
    parser = argparse.ArgumentParser(
        description="Normalize a Granola local cache snapshot and submit it through loom meetings import."
    )
    parser.add_argument("--store", required=True)
    parser.add_argument("--workspace", required=True)
    parser.add_argument("--cache", nargs="+")
    parser.add_argument("--supabase")
    parser.add_argument("--supabase-enc")
    parser.add_argument("--storage-dek")
    parser.add_argument("--keychain-service", default="Granola Safe Storage")
    parser.add_argument("--api-url", default="https://api.granola.ai/v2/get-documents")
    parser.add_argument(
        "--transcript-api-url",
        default="https://api.granola.ai/v1/get-document-transcript",
    )
    parser.add_argument("--include-transcripts", action="store_true")
    parser.add_argument("--loom", default="loom")
    parser.add_argument("--source-scope")
    parser.add_argument("--observed-at", type=int)
    parser.add_argument("--snapshot-out")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--report-format", default="text", choices=["text", "json"])
    return parser.parse_args()


def sha256_digest(value):
    if isinstance(value, bytes):
        data = value
    else:
        data = json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    return "sha256:" + hashlib.sha256(data).hexdigest()


def load_cache_bytes(raw):
    payload = json.loads(raw)
    cache = payload.get("cache", payload)
    if isinstance(cache, str):
        cache = json.loads(cache)
    if not isinstance(cache, dict):
        raise ValueError("Granola cache must be a JSON object or a top-level cache object")
    return cache


def iter_cache_inputs(paths):
    for value in paths:
        path = Path(value)
        if path.is_dir():
            for child in sorted(path.rglob("*.json")):
                raw = child.read_bytes()
                yield str(child), raw, load_cache_bytes(raw)
        elif zipfile.is_zipfile(path):
            with zipfile.ZipFile(path) as archive:
                for name in sorted(archive.namelist()):
                    if name.endswith("/") or not name.lower().endswith(".json"):
                        continue
                    raw = archive.read(name)
                    yield f"{path}:{name}", raw, load_cache_bytes(raw)
        elif tarfile.is_tarfile(path):
            with tarfile.open(path) as archive:
                for member in sorted(archive.getmembers(), key=lambda item: item.name):
                    if not member.isfile() or not member.name.lower().endswith(".json"):
                        continue
                    extracted = archive.extractfile(member)
                    if extracted is None:
                        continue
                    raw = extracted.read()
                    yield f"{path}:{member.name}", raw, load_cache_bytes(raw)
        elif path.suffix == ".gz":
            raw = gzip.decompress(path.read_bytes())
            yield str(path), raw, load_cache_bytes(raw)
        else:
            raw = path.read_bytes()
            yield str(path), raw, load_cache_bytes(raw)


def as_records(value):
    if value is None:
        return []
    if isinstance(value, list):
        return [item for item in value if isinstance(item, dict)]
    if isinstance(value, dict):
        records = []
        for key, item in value.items():
            if isinstance(item, dict):
                copy = dict(item)
                copy.setdefault("id", key)
                records.append(copy)
        return records
    return []


def first_value(record, names):
    for name in names:
        value = record.get(name)
        if value not in (None, ""):
            return value
    return None


def optional_int(value):
    if value in (None, ""):
        return None
    if isinstance(value, bool):
        return None
    if isinstance(value, (int, float)):
        return int(value)
    if isinstance(value, str) and value.isdigit():
        return int(value)
    return None


def string_values(value):
    if value in (None, ""):
        return []
    if isinstance(value, str):
        return [value]
    if not isinstance(value, list):
        return []
    out = []
    for item in value:
        if isinstance(item, str):
            out.append(item)
        elif isinstance(item, dict):
            label = first_value(item, ["id", "email", "name", "displayName"])
            if label is not None:
                out.append(str(label))
    return out


def transcript_spans(source_entity_id, value):
    spans = []
    for index, item in enumerate(as_records(value)):
        text = first_value(item, ["text", "content", "transcript", "body"])
        if text in (None, ""):
            continue
        span = {
            "span_id": f"span/{source_entity_id}/transcript/{first_value(item, ['id', 'span_id']) or index}",
            "locator": first_value(item, ["locator"]) or f"transcript/{index}",
            "text": str(text),
        }
        speaker = first_value(item, ["speaker", "speakerName", "speaker_label", "speakerLabel"])
        if speaker not in (None, ""):
            span["speaker"] = str(speaker)
        language = first_value(item, ["language", "lang"])
        if language not in (None, ""):
            span["language"] = str(language)
        spans.append(span)
    return spans


def build_snapshot(cache_path, raw, cache, source_scope, observed_at):
    state = cache.get("state", cache)
    documents = []
    documents.extend(as_records(state.get("documents")))
    documents.extend(as_records(state.get("sharedDocuments")))
    if not documents:
        documents.extend(as_records(state.get("notes")))
    transcripts = state.get("transcripts", {})
    if transcripts is None:
        transcripts = {}
    if not isinstance(transcripts, dict):
        transcripts = {}

    items = []
    coverage_gaps = []
    for document in documents:
        source_entity_id = first_value(document, ["id", "note_id", "noteId", "document_id", "documentId"])
        if source_entity_id in (None, ""):
            raise ValueError("Granola cache document is missing a stable note id")
        source_entity_id = str(source_entity_id)
        transcript = document.get("transcript")
        if transcript is None:
            transcript = transcripts.get(source_entity_id)
        spans = transcript_spans(source_entity_id, transcript)
        if not spans:
            coverage_gaps.append(f"missing-transcript:{source_entity_id}")
        source_created_at = optional_int(
            first_value(document, ["created_at", "createdAt", "created_time", "createdTime"])
        )
        source_updated_at = optional_int(
            first_value(document, ["updated_at", "updatedAt", "lastModified", "modifiedAt"])
        )
        item = {
            "source_entity_id": source_entity_id,
            "source_digest": sha256_digest({"document": document, "transcript": transcript}),
            "source_sidecar_digest": sha256_digest({"document": document, "transcript": transcript}),
            "source_sidecar": {"document": document, "transcript": transcript},
            "meeting_id": f"meeting/{source_entity_id}",
            "title": first_value(document, ["title", "name"]) or f"Untitled meeting {source_entity_id}",
            "transcript_spans": spans,
        }
        if source_created_at is not None:
            item["source_created_at"] = source_created_at
        if source_updated_at is not None:
            item["source_updated_at"] = source_updated_at
        owner = first_value(document, ["owner", "ownerEmail", "owner_email"])
        if owner not in (None, ""):
            item["owner"] = str(owner)
        attendees = string_values(first_value(document, ["attendees", "participants"]))
        if attendees:
            item["attendees"] = attendees
        folders = string_values(first_value(document, ["folder_refs", "folderRefs", "folder_ids", "folderIds"]))
        folder = first_value(document, ["folder_id", "folderId"])
        if folder not in (None, ""):
            folders.append(str(folder))
        if folders:
            item["folder_refs"] = sorted(set(folders))
        calendar_event = first_value(document, ["calendar_event", "calendarEvent", "calendar_event_id"])
        if calendar_event not in (None, ""):
            item["calendar_event"] = str(calendar_event)
        summary = first_value(document, ["summary_text", "summaryText", "summary", "notes"])
        if summary not in (None, ""):
            item["summary_text"] = str(summary)
        markdown_digest = first_value(document, ["summary_markdown_digest", "summaryMarkdownDigest"])
        if markdown_digest not in (None, ""):
            item["summary_markdown_digest"] = str(markdown_digest)
        items.append(item)

    return {
        "snapshot_version": 1,
        "profile": "meetings",
        "source_system": "granola_mixed",
        "source_scope": source_scope or f"granola-cache:{cache_path}",
        "observed_at": observed_at or int(time.time() * 1000),
        "coverage": "partial",
        "source_sidecar_digest": sha256_digest(raw),
        "coverage_gaps": coverage_gaps,
        "items": sorted(items, key=lambda item: item["source_entity_id"]),
    }


def extract_access_token(path):
    payload = json.loads(Path(path).read_text())
    return access_token_from_payload(payload)


def access_token_from_payload(payload):
    tokens = payload.get("workos_tokens")
    if isinstance(tokens, str):
        tokens = json.loads(tokens)
    if not isinstance(tokens, dict):
        raise ValueError("supabase.json is missing workos_tokens")
    token = tokens.get("access_token")
    if not isinstance(token, str) or not token:
        raise ValueError("supabase.json is missing workos_tokens.access_token")
    return token


def extract_access_token_from_encrypted(supabase_enc_path, storage_dek_path, service):
    payload = json.loads(decrypt_granola_file(supabase_enc_path, storage_dek_path, service))
    return access_token_from_payload(payload)


def decrypt_granola_file(path, storage_dek_path, service):
    dek = unwrap_storage_dek(storage_dek_path, service)
    payload = Path(path).read_bytes()
    if len(payload) < 29:
        raise ValueError(f"encrypted Granola file is too short: {path}")
    iv = payload[:12]
    ciphertext_and_tag = payload[12:]
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM

    plaintext = AESGCM(dek).decrypt(iv, ciphertext_and_tag, None)
    if plaintext.startswith(b"\x1f\x8b"):
        plaintext = gzip.decompress(plaintext)
    return plaintext.decode("utf-8")


def unwrap_storage_dek(storage_dek_path, service):
    wrapped = Path(storage_dek_path).read_bytes()
    if not wrapped.startswith(b"v10"):
        text = wrapped.strip()
        try:
            import base64

            decoded = base64.b64decode(text)
        except Exception:
            decoded = b""
        if decoded.startswith(b"v10"):
            wrapped = decoded
        else:
            raise ValueError("storage.dek is not an Electron safeStorage v10 blob")
    secret = keychain_secret(service)
    from cryptography.hazmat.primitives import hashes, padding
    from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
    from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2HMAC

    kdf = PBKDF2HMAC(
        algorithm=hashes.SHA1(),
        length=16,
        salt=b"saltysalt",
        iterations=1003,
    )
    key = kdf.derive(secret.encode("utf-8"))
    decryptor = Cipher(algorithms.AES(key), modes.CBC(b" " * 16)).decryptor()
    padded = decryptor.update(wrapped[3:]) + decryptor.finalize()
    unpadder = padding.PKCS7(128).unpadder()
    dek = unpadder.update(padded) + unpadder.finalize()
    if len(dek) != 32:
        import base64

        try:
            decoded = base64.b64decode(dek, validate=True)
        except Exception:
            decoded = b""
        if len(decoded) == 32:
            dek = decoded
    if len(dek) != 32:
        raise ValueError(f"unwrapped Granola DEK has {len(dek)} bytes, expected 32")
    return dek


def keychain_secret(service):
    completed = subprocess.run(
        ["security", "find-generic-password", "-s", service, "-w"],
        stdout=PIPE,
        stderr=PIPE,
        check=False,
        text=True,
    )
    if completed.returncode != 0:
        raise ValueError(f"could not read macOS Keychain service {service!r}")
    return completed.stdout.rstrip("\n")


def api_documents(api_url, token):
    documents = []
    offset = 0
    limit = 100
    while True:
        body = json.dumps(
            {
                "limit": limit,
                "offset": offset,
                "include_last_viewed_panel": True,
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            api_url,
            data=body,
            method="POST",
            headers={
                "Authorization": f"Bearer {token}",
                "Accept": "*/*",
                "User-Agent": "Granola/5.354.0",
                "X-Client-Version": "5.354.0",
                "Content-Type": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=120) as response:
                payload = read_json_response(response)
        except urllib.error.HTTPError as exc:
            preview = exc.read(200).decode("utf-8", "replace")
            raise ValueError(f"Granola API request failed: {exc.code} {preview}") from exc
        page = payload.get("docs", [])
        if not isinstance(page, list):
            raise ValueError("Granola API response is missing docs array")
        documents.extend(item for item in page if isinstance(item, dict))
        if len(page) < limit:
            break
        offset += limit
    return documents


def api_transcript(transcript_api_url, token, document_id):
    body = json.dumps({"document_id": document_id}).encode("utf-8")
    request = urllib.request.Request(
        transcript_api_url,
        data=body,
        method="POST",
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "*/*",
            "User-Agent": "Granola/5.354.0",
            "X-Client-Version": "5.354.0",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            payload = read_json_response(response)
    except urllib.error.HTTPError:
        return None
    if isinstance(payload, dict):
        for key in ["transcript", "transcript_items", "segments", "items"]:
            value = payload.get(key)
            if isinstance(value, list):
                return value
    if isinstance(payload, list):
        return payload
    return None


def read_json_response(response):
    data = response.read()
    if data.startswith(b"\x1f\x8b"):
        data = gzip.decompress(data)
    return json.loads(data)


def prosemirror_text(value):
    if isinstance(value, str):
        try:
            value = json.loads(value)
        except json.JSONDecodeError:
            return value
    lines = []

    def walk(node):
        if isinstance(node, dict):
            text = node.get("text")
            if isinstance(text, str):
                lines.append(text)
            for child in node.get("content", []) or []:
                walk(child)
            if node.get("type") in {"paragraph", "heading", "listItem"}:
                lines.append("\n")
        elif isinstance(node, list):
            for child in node:
                walk(child)

    walk(value)
    return " ".join("".join(lines).split())


def document_summary(document):
    notes_plain = document.get("notes_plain")
    if isinstance(notes_plain, str) and notes_plain.strip():
        return notes_plain.strip()
    notes = document.get("notes")
    summary = prosemirror_text(notes)
    if summary:
        return summary
    panel = document.get("last_viewed_panel")
    if isinstance(panel, dict):
        panel_content = prosemirror_text(panel.get("content"))
        if panel_content:
            return panel_content
        original = panel.get("original_content")
        if isinstance(original, str) and original.strip():
            return original.strip()
    content = document.get("content")
    if isinstance(content, str) and content.strip():
        return content.strip()
    return None


def build_api_snapshot(api_url, transcript_api_url, token, include_transcripts, source_scope, observed_at):
    documents = api_documents(api_url, token)
    items = []
    coverage_gaps = []
    for document in documents:
        source_entity_id = document.get("id")
        if not isinstance(source_entity_id, str) or not source_entity_id:
            coverage_gaps.append("missing-id")
            continue
        summary = document_summary(document)
        item = {
            "source_entity_id": source_entity_id,
            "source_digest": sha256_digest(document),
            "source_sidecar_digest": sha256_digest(document),
            "source_sidecar": document,
            "meeting_id": f"meeting/{source_entity_id}",
            "title": document.get("title") or f"Untitled meeting {source_entity_id}",
        }
        created_at = document.get("created_at")
        if isinstance(created_at, str):
            item["source_created_at"] = iso_millis(created_at)
        updated_at = document.get("updated_at")
        if isinstance(updated_at, str):
            item["source_updated_at"] = iso_millis(updated_at)
        if summary:
            item["summary_text"] = summary
        else:
            coverage_gaps.append(f"missing-summary:{source_entity_id}")
        if include_transcripts:
            transcript = api_transcript(transcript_api_url, token, source_entity_id)
            spans = transcript_spans(source_entity_id, transcript)
            if spans:
                item["transcript_spans"] = spans
            else:
                coverage_gaps.append(f"missing-transcript:{source_entity_id}")
        items.append(item)
    return {
        "snapshot_version": 1,
        "profile": "granola-api",
        "source_system": "granola_api",
        "source_scope": source_scope or "granola-api:get-documents",
        "observed_at": observed_at,
        "coverage": "partial",
        "source_sidecar_digest": sha256_digest(documents),
        "coverage_gaps": sorted(set(coverage_gaps)),
        "items": sorted(items, key=lambda item: item["source_entity_id"]),
    }


def iso_millis(value):
    from datetime import datetime

    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None
    return int(parsed.timestamp() * 1000)


def combine_snapshots(snapshots, source_scope, observed_at):
    if not snapshots:
        raise ValueError("no Granola cache snapshots found")
    if len(snapshots) == 1:
        snapshot = snapshots[0]
        if source_scope:
            snapshot["source_scope"] = source_scope
        return snapshot
    items = {}
    coverage_gaps = []
    sidecars = []
    for snapshot in snapshots:
        for item in snapshot["items"]:
            items[item["source_entity_id"]] = item
        coverage_gaps.extend(snapshot.get("coverage_gaps", []))
        sidecars.append(
            {
                "source_scope": snapshot["source_scope"],
                "source_sidecar_digest": snapshot["source_sidecar_digest"],
            }
        )
    return {
        "snapshot_version": 1,
        "profile": "granola-app",
        "source_system": "granola_cache",
        "source_scope": source_scope or f"granola-cache-batch:{len(snapshots)}",
        "observed_at": observed_at or int(time.time() * 1000),
        "coverage": "partial",
        "source_sidecar_digest": sha256_digest(sidecars),
        "coverage_gaps": sorted(set(coverage_gaps)),
        "items": [items[key] for key in sorted(items)],
    }


def write_snapshot(snapshot, path):
    data = json.dumps(snapshot, sort_keys=True, indent=2).encode("utf-8")
    if path:
        Path(path).write_bytes(data + b"\n")
    return data


def run_import(args, snapshot, snapshot_bytes):
    with tempfile.NamedTemporaryFile("wb", suffix=".meetings.json", delete=False) as handle:
        handle.write(snapshot_bytes)
        handle.write(b"\n")
        snapshot_path = handle.name
    command = [
        args.loom,
        "meetings",
        "import",
        args.store,
        args.workspace,
        "--input-profile",
        input_profile_for_snapshot(snapshot),
        "--input",
        snapshot_path,
        "--report-format",
        args.report_format,
    ]
    if args.dry_run:
        command.append("--dry-run")
    try:
        return subprocess.run(command, check=False).returncode
    finally:
        Path(snapshot_path).unlink(missing_ok=True)


def input_profile_for_snapshot(snapshot):
    profile = snapshot.get("profile")
    if profile in {"granola-api", "granola-app", "granola-mcp", "csv", "generic"}:
        return profile
    return "generic"


def main():
    args = parse_args()
    if not args.cache and not args.supabase:
        if not args.supabase_enc:
            raise ValueError("provide --cache, --supabase, --supabase-enc, or a combination")
    observed_at = args.observed_at or int(time.time() * 1000)
    snapshots = []
    if args.cache:
        snapshots.extend(
            build_snapshot(label, raw, cache, None, observed_at)
            for label, raw, cache in iter_cache_inputs(args.cache)
        )
    if args.supabase:
        token = extract_access_token(args.supabase)
        snapshots.append(
            build_api_snapshot(
                args.api_url,
                args.transcript_api_url,
                token,
                args.include_transcripts,
                None,
                observed_at,
            )
        )
    if args.supabase_enc:
        storage_dek = args.storage_dek
        if storage_dek is None:
            storage_dek = str(Path(args.supabase_enc).with_name("storage.dek"))
        token = extract_access_token_from_encrypted(args.supabase_enc, storage_dek, args.keychain_service)
        snapshots.append(
            build_api_snapshot(
                args.api_url,
                args.transcript_api_url,
                token,
                args.include_transcripts,
                None,
                observed_at,
            )
        )
    snapshot = combine_snapshots(snapshots, args.source_scope, observed_at)
    snapshot_bytes = write_snapshot(snapshot, args.snapshot_out)
    return run_import(args, snapshot, snapshot_bytes)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"granola-cache-import: {exc}", file=sys.stderr)
        raise SystemExit(2)
