import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const loom = require("./index.js");

const path = join(mkdtempSync(join(tmpdir(), "loom-drive-generated-")), "drive.loom");
loom.createLoom(path, "default", null, null);
loom.workspaceCreate(path, "studio", "vcs");

const root = JSON.parse(loom.driveListJson(path, "studio", "drive-main", "root"));
assert.equal(root.folder_id, "root");
assert.deepEqual(root.entries, []);

const folderA = JSON.parse(
  loom.driveCreateFolderJson(path, "studio", "drive-main", "root", "folder-a", "A", root.profile_root),
);
assert.equal(folderA.target_entity_id, "folder-a");
assert.throws(
  () => loom.driveCreateFolderJson(path, "studio", "drive-main", "root", "stale", "Stale", root.profile_root),
  /CONFLICT|expected_root|profile root/i,
);

const renamed = JSON.parse(
  loom.driveRenameJson(
    path,
    "studio",
    "drive-main",
    "root",
    "folder-a",
    "A2",
    folderA.profile_root,
  ),
);
assert.equal(renamed.target_entity_id, "folder-a");
const stat = JSON.parse(loom.driveStatJson(path, "studio", "drive-main", "root", "A2"));
assert.equal(stat.node_id, "folder-a");

const folderB = JSON.parse(
  loom.driveCreateFolderJson(path, "studio", "drive-main", "root", "folder-b", "B", renamed.profile_root),
);
const moved = JSON.parse(
  loom.driveMoveJson(
    path,
    "studio",
    "drive-main",
    "root",
    "folder-b",
    "folder-a",
    folderB.profile_root,
  ),
);
const heldDelete = JSON.parse(
  loom.driveDeleteJson(path, "studio", "drive-main", "folder-b", "folder-a", renamed.profile_root),
);
assert.equal(heldDelete.operation_kind, "folder.delete_held");
assert.equal(JSON.parse(loom.driveListConflictsJson(path, "studio", "drive-main")).length, 1);
const resolved = JSON.parse(
  loom.driveResolveConflictJson(path, "studio", "drive-main", heldDelete.conflict_id, "keep_current"),
);
assert.equal(resolved.operation_kind, "conflict.resolved");
assert.ok(
  JSON.parse(loom.driveListConflictsJson(path, "studio", "drive-main")).some(
    (conflict) => conflict.conflict_id === heldDelete.conflict_id && conflict.resolution === "keep_current",
  ),
);
const deleted = JSON.parse(
  loom.driveDeleteJson(path, "studio", "drive-main", "folder-b", "folder-a", moved.profile_root),
);
assert.equal(deleted.target_entity_id, "folder-a");

const upload = JSON.parse(
  loom.driveCreateUploadJson(
    path,
    "studio",
    "drive-main",
    "upload-1",
    "root",
    "nul.bin",
    "file-1",
    deleted.profile_root,
    1000n,
    false,
  ),
);
assert.equal(upload.upload_id, "upload-1");
const payload = Buffer.from([0x64, 0x72, 0x69, 0x76, 0x65, 0x00, 0x62, 0x79, 0x74, 0x65, 0x73]);
loom.driveUploadChunkJson(path, "studio", "drive-main", "upload-1", payload);
const committed = JSON.parse(loom.driveCommitUploadJson(path, "studio", "drive-main", "upload-1"));
assert.equal(committed.target_entity_id, "file-1");
assert.deepEqual(Buffer.from(loom.driveReadFile(path, "studio", "drive-main", "file-1")), payload);
assert.equal(JSON.parse(loom.driveListVersionsJson(path, "studio", "drive-main", "file-1")).length, 1);
assert.ok(JSON.parse(loom.driveListConflictsJson(path, "studio", "drive-main")).length >= 1);

loom.driveGrantShareJson(
  path,
  "studio",
  "drive-main",
  "grant-1",
  "file",
  "file-1",
  "05050505-0505-4505-8505-050505050505",
  "editor",
  2000n,
  2500n,
);
assert.equal(JSON.parse(loom.driveListSharesJson(path, "studio", "drive-main")).length, 1);
const shareNoOp = JSON.parse(loom.driveApplyShareExpiryJson(path, "studio", "drive-main", 2100n));
assert.equal(shareNoOp.remaining_grants, 1);
const revoked = JSON.parse(loom.driveRevokeShareJson(path, "studio", "drive-main", "grant-1"));
assert.equal(revoked.operation_kind, "share.revoked");
assert.equal(JSON.parse(loom.driveListSharesJson(path, "studio", "drive-main")).length, 0);
loom.driveGrantShareJson(
  path,
  "studio",
  "drive-main",
  "grant-expiring",
  "file",
  "file-1",
  "05050505-0505-4505-8505-050505050505",
  "viewer",
  2200n,
  2300n,
);
const expiredShare = JSON.parse(loom.driveApplyShareExpiryJson(path, "studio", "drive-main", 2300n));
assert.deepEqual(expiredShare.expired_grant_ids, ["grant-expiring"]);
assert.equal(JSON.parse(loom.driveListSharesJson(path, "studio", "drive-main")).length, 0);
loom.drivePinRetentionJson(
  path,
  "studio",
  "drive-main",
  "pin-1",
  "legal_hold",
  committed.profile_root,
  "file:file-1",
  3000n,
  null,
);
assert.equal(JSON.parse(loom.driveListRetentionJson(path, "studio", "drive-main")).length, 1);
const retentionNoOp = JSON.parse(loom.driveApplyRetentionJson(path, "studio", "drive-main", 3100n));
assert.equal(retentionNoOp.remaining_pins, 1);
const unpinned = JSON.parse(loom.driveUnpinRetentionJson(path, "studio", "drive-main", "pin-1"));
assert.equal(unpinned.operation_kind, "retention.unpinned");
assert.equal(JSON.parse(loom.driveListRetentionJson(path, "studio", "drive-main")).length, 0);
loom.drivePinRetentionJson(
  path,
  "studio",
  "drive-main",
  "pin-expiring",
  "trash_subtree",
  committed.profile_root,
  "file:file-1",
  3200n,
  3300n,
);
const expiredRetention = JSON.parse(loom.driveApplyRetentionJson(path, "studio", "drive-main", 3300n));
assert.deepEqual(expiredRetention.expired_pin_ids, ["pin-expiring"]);
assert.equal(JSON.parse(loom.driveListRetentionJson(path, "studio", "drive-main")).length, 0);

const reopened = JSON.parse(loom.driveListJson(path, "studio", "drive-main", "root"));
assert.ok(reopened.entries.some((entry) => entry.node_id === "file-1"));
