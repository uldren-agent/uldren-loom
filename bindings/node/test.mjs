// Node binding tests for Uldren Loom. Runs the cross-language result vector through the typed
// exec path and asserts byte-for-byte equality with the engine-pinned shared fixture - identical
// canonical CBOR means identical typed values across all eight bindings.
import assert from "node:assert/strict";
import { readFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const loom = require("./index.js");

const here = fileURLToPath(new URL(".", import.meta.url));
const fixture = JSON.parse(
  readFileSync(join(here, "..", "conformance", "result-vectors.json"), "utf8"),
);

// Smoke: version + blob digest shape.
assert.notEqual(loom.version(), "");
assert.match(loom.blobDigest(Buffer.from("abc")), /^blake3:[0-9a-f]{64}$/);

// Smoke: capability report is a non-empty canonical-CBOR buffer (decoded shape is covered by the
// core and FFI tests). The CBOR text strings appear verbatim in the buffer, so check a couple.
const caps = Buffer.from(loom.capabilities());
assert.ok(caps.length > 0, "capabilities() returns a non-empty buffer");
assert.ok(caps.includes(Buffer.from("object-store")), "report lists object-store");
assert.ok(caps.includes(Buffer.from("sql")), "report lists sql");
assert.ok(caps.includes(Buffer.from("lanes")), "report lists lane capability state");
for (const name of [
  "lanesCreate",
  "lanesGet",
  "lanesList",
  "lanesStatusReportUpdate",
  "lanesReviewerFeedbackUpdate",
  "lanesTicketAdd",
  "lanesTicketRemove",
]) {
  assert.equal(typeof loom[name], "function", `${name} is exported`);
}
const runtimeProfile = Buffer.from(loom.runtimeProfile());
assert.ok(runtimeProfile.length > 0, "runtimeProfile() returns a non-empty buffer");
assert.ok(runtimeProfile.includes(Buffer.from("binary_channel")), "profile lists binary channel");
assert.ok(runtimeProfile.includes(Buffer.from("crypto_provider")), "profile lists crypto provider");
const surfaceCatalog = JSON.parse(loom.studioSurfaceCatalogJson("studio", "core"));
assert.equal(surfaceCatalog.workspace, "studio");
assert.equal(surfaceCatalog.set, "core");
assert.ok(surfaceCatalog.apps.some((app) => app.app_id === "ticket-details"));
assert.throws(() => loom.studioSurfaceCatalogJson("studio", "bogus"), /unsupported Studio surface catalog set/);

const meetingsPath = join(mkdtempSync(join(tmpdir(), "loom-")), "meetings.loom");
loom.createLoom(meetingsPath, "default", null, null);
loom.workspaceCreate(meetingsPath, "studio", "vcs");
const meetingsSnapshot = Buffer.from(
  JSON.stringify({
    snapshot_version: 1,
    profile: "granola-app",
    source_system: "granola-app",
    source_scope: "local-cache",
    observed_at: 500,
    coverage: "complete",
    items: [
      {
        source_entity_id: "note-1",
        source_digest: `blake3:${"0".repeat(64)}`,
        source_sidecar: { id: "note-1", raw: true },
        title: "Planning",
        summary_text: "Planning summary",
        transcript_spans: [{ text: "Capture decisions." }],
        decisions: [{ label: "Use normalized meeting imports." }],
      },
    ],
  }),
);
const meetingsReport = JSON.parse(
  loom.meetingsImportSnapshot(meetingsPath, "studio", "granola-app", meetingsSnapshot, false),
);
assert.equal(meetingsReport.profile, "meetings");
assert.equal(meetingsReport.rows_imported, 1);
assert.equal(Buffer.from(loom.meetingsSourceRead(meetingsPath, "studio", "note-1", "summary.txt")).toString(), "Planning summary");

const drivePath = join(mkdtempSync(join(tmpdir(), "loom-")), "drive.loom");
loom.createLoom(drivePath, "default", null, null);
loom.workspaceCreate(drivePath, "studio", "vcs");
const driveRoot = JSON.parse(loom.driveListJson(drivePath, "studio", "drive-main", "root"));
assert.equal(driveRoot.folder_id, "root");
assert.deepEqual(driveRoot.entries, []);
loom.driveCreateFolderJson(drivePath, "studio", "drive-main", "root", "folder-1", "Specs", driveRoot.profile_root);
const upload = JSON.parse(
  loom.driveCreateUploadJson(
    drivePath,
    "studio",
    "drive-main",
    "upload-1",
    "folder-1",
    "readme.txt",
    "file-1",
    JSON.parse(loom.driveListJson(drivePath, "studio", "drive-main", "root")).profile_root,
    1000n,
    false,
  ),
);
assert.equal(upload.upload_id, "upload-1");
loom.driveUploadChunkJson(drivePath, "studio", "drive-main", "upload-1", Buffer.from("drive bytes"));
const committed = JSON.parse(loom.driveCommitUploadJson(drivePath, "studio", "drive-main", "upload-1"));
assert.equal(committed.target_entity_id, "file-1");
assert.equal(Buffer.from(loom.driveReadFile(drivePath, "studio", "drive-main", "file-1")).toString(), "drive bytes");
assert.equal(JSON.parse(loom.driveListVersionsJson(drivePath, "studio", "drive-main", "file-1")).length, 1);
loom.driveGrantShareJson(
  drivePath,
  "studio",
  "drive-main",
  "grant-1",
  "file",
  "file-1",
  "05050505-0505-4505-8505-050505050505",
  "editor",
  2000n,
  null,
);
assert.equal(JSON.parse(loom.driveListSharesJson(drivePath, "studio", "drive-main")).length, 1);
loom.drivePinRetentionJson(
  drivePath,
  "studio",
  "drive-main",
  "pin-1",
  "legal_hold",
  committed.profile_root,
  "file-1",
  3000n,
  null,
);
assert.equal(JSON.parse(loom.driveListRetentionJson(drivePath, "studio", "drive-main")).length, 1);

// Watch: a broad subscription over a SQL workspace returns the canonical watch batch bytes.
const watchPath = join(mkdtempSync(join(tmpdir(), "loom-")), "watch.loom");
loom.createLoom(watchPath, "default", null, null);
const watchDb = new loom.LoomSql(watchPath, "watchapp", "main");
watchDb.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
watchDb.exec("INSERT INTO t VALUES (1, 'a')");
const watchCursor = loom.watchSubscribe(watchPath, "watchapp", "main", null, null, null, null);
assert.match(watchDb.commit("seed", "node"), /^blake3:[0-9a-f]{64}$/);
const watchBatch = Buffer.from(loom.watchPoll(watchPath, watchCursor, 10, null));
assert.ok(watchBatch.includes(Buffer.from("loom.watch.batch.v1")), "watch batch schema");
assert.ok(watchBatch.includes(Buffer.from("unsupported_domains")), "watch reports unsupported domains");
assert.ok(watchBatch.includes(Buffer.from("sql")), "watch reports sql domain");

// Cross-language exec vector.
const vec = fixture.vectors.result_exec_select;
const path = join(mkdtempSync(join(tmpdir(), "loom-")), "vec.loom");
const db = new loom.LoomSql(path, "app", "main");
db.exec(vec.sql[0]); // CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)
db.exec(vec.sql[1]); // INSERT INTO t VALUES (1, 'hi'), (2, NULL)

// Raw canonical bytes must equal the fixture exactly.
const raw = Buffer.from(db.execBytes(vec.exec_sql)).toString("hex");
assert.equal(raw, vec.canonical_hex, "node execBytes drifted from the shared vector");

// Typed exec: id is a 64-bit integer (BigInt), text stays a string, NULL is null.
const payloads = db.exec(vec.exec_sql);
assert.equal(payloads[0].kind, "select");
assert.deepEqual(payloads[0].rows, [
  [1n, "hi"],
  [2n, null],
]);

// JSON/debug form exposes the same select payload.
const debugJson = db.execJson(vec.exec_sql);
assert.ok(JSON.parse(debugJson).length >= 1);
assert.match(debugJson, /hi/);

// query(): a SELECT's rows as a natively-iterable typed array (streaming form).
const qpath = join(mkdtempSync(join(tmpdir(), "loom-")), "query.loom");
const qdb = new loom.LoomSql(qpath, "app", "main");
qdb.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
qdb.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')");
const collected = [];
for (const row of qdb.query("SELECT id, v FROM t ORDER BY id")) collected.push(row);
assert.deepEqual(collected, [
  [1n, "a"],
  [2n, "b"],
]);

// Document binding parity: text uses strings and binary uses explicit byte APIs.
const docPath = join(mkdtempSync(join(tmpdir(), "loom-")), "docs.loom");
loom.createLoom(docPath, "default", null, null);
const textDigest = loom.docPutText(docPath, "docs", "notes", "a", "hello text", null, null);
assert.match(textDigest, /^blake3:[0-9a-f]{64}$/);
const textDoc = loom.docGetText(docPath, "docs", "notes", "a", null);
assert.deepEqual(textDoc, { text: "hello text", digest: textDigest });
assert.equal(loom.docGetText(docPath, "docs", "notes", "missing", null), null);
assert.throws(
  () => loom.docPutText(docPath, "docs", "notes", "a", "stale", loom.blobDigest(Buffer.from("stale")), null),
  /CAS_MISMATCH/,
);
const updatedDigest = loom.docPutText(docPath, "docs", "notes", "a", "updated text", textDigest, null);
assert.notEqual(updatedDigest, textDigest);
const binaryDigest = loom.docPutBinary(docPath, "docs", "notes", "raw", Uint8Array.from([0xff, 0x00]), null, null);
assert.match(binaryDigest, /^blake3:[0-9a-f]{64}$/);
const binaryDoc = loom.docGetBinary(docPath, "docs", "notes", "raw", null);
assert.deepEqual(Array.from(binaryDoc.bytes), [0xff, 0x00]);
assert.equal(binaryDoc.digest, binaryDigest);
assert.ok(Buffer.from(loom.docListBinary(docPath, "docs", "notes", null)).length > 0);
assert.throws(
  () => loom.docGetText(docPath, "docs", "notes", "raw", null),
  /DOCUMENT_NOT_TEXT/,
);

// Batch + SQL transaction: a rolled-back insert vanishes; a pre-BEGIN row survives; one atomic commit.
const bpath = join(mkdtempSync(join(tmpdir(), "loom-")), "batch.loom");
const batch = new loom.LoomSqlBatch(bpath, "app", "main");
batch.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
batch.exec("INSERT INTO t VALUES (1, 'a')");
batch.exec("BEGIN");
batch.exec("INSERT INTO t VALUES (2, 'b')");
batch.exec("ROLLBACK");
batch.commit();
const rows = batch.exec("SELECT v FROM t ORDER BY id")[0].rows;
assert.deepEqual(rows, [["a"]], "rolled-back row must not survive");
// Committing the batch with an open transaction is rejected.
batch.exec("BEGIN");
assert.throws(() => batch.commit(), /open SQL transaction/);
batch.exec("ROLLBACK");
batch.close();

// Multi-wrap add/remove: add a second passphrase and a raw KEK, then remove the original wrap.
const kpath = join(mkdtempSync(join(tmpdir(), "loom-")), "keys.loom");
loom.createLoom(kpath, "default", null, "first-pass");
// A second passphrase wrap opens the same store.
loom.keyAddWrapKeyed(kpath, "first-pass", "second-pass", false);
assert.equal(loom.workspaceListJson(kpath, "second-pass"), "[]");
// Re-adding an existing credential is rejected, not a silent no-op.
assert.throws(() => loom.keyAddWrapKeyed(kpath, "first-pass", "second-pass", false), /already exists/);
// A raw 256-bit KEK wrap; a wrong length is rejected.
loom.keyAddWrapWithKek(kpath, "first-pass", Buffer.alloc(32, 0x5a), false);
assert.throws(() => loom.keyAddWrapWithKek(kpath, "first-pass", Buffer.alloc(16), false), /32 bytes/);
// Removing one passphrase wrap is allowed while another passphrase recovery wrap remains.
loom.keyRemoveWrap(kpath, "first-pass", 0, false);
assert.throws(() => loom.workspaceListJson(kpath, "first-pass"));
assert.equal(loom.workspaceListJson(kpath, "second-pass"), "[]");

// Local identity and ACL management: unauthenticated root bootstrap, then per-call root auth.
const authPath = join(mkdtempSync(join(tmpdir(), "loom-")), "auth.loom");
loom.createLoom(authPath, "default", null, null);
const bootstrapIdentity = JSON.parse(loom.identityListJson(authPath));
assert.equal(bootstrapIdentity.authenticated_mode, false);
const rootId = bootstrapIdentity.root;
const adminRoleId = bootstrapIdentity.roles.find((r) => r.name === "admin").id;
assert.ok(bootstrapIdentity.principals.some((p) => p.id === rootId && p.roles.includes(adminRoleId)));
loom.workspaceCreate(authPath, "policy", "vcs");
loom.identitySetPassphrase(authPath, rootId, "root-pass");
assert.throws(() => loom.identityListJson(authPath));
loom.authenticatePassphrase(authPath, rootId, "root-pass");
const aliceId = loom.identityAddPrincipal(authPath, "alice", "Alice", "user", null, rootId, "root-pass");
loom.identitySetPassphrase(authPath, aliceId, "alice-pass", null, rootId, "root-pass");
loom.identityAssignRole(authPath, aliceId, adminRoleId, null, rootId, "root-pass");
const authIdentity = JSON.parse(loom.identityListJson(authPath, null, rootId, "root-pass"));
assert.equal(authIdentity.authenticated_mode, true);
assert.ok(authIdentity.principals.some((p) => p.id === aliceId && p.has_passphrase && p.roles.includes(adminRoleId)));
assert.deepEqual(authIdentity.app_credentials, []);
assert.deepEqual(authIdentity.external_credentials, []);
assert.deepEqual(authIdentity.public_keys, []);
const externalId = loom.identityCreateExternalCredential(
  authPath,
  aliceId,
  "oidc-subject",
  "okta-prod",
  "https://issuer.example",
  "00u123",
  "sha256:metadata",
  null,
  rootId,
  "root-pass",
);
const externalIdentity = JSON.parse(loom.identityListJson(authPath, null, rootId, "root-pass"));
assert.ok(
  externalIdentity.external_credentials.some(
    (credential) =>
      credential.id === externalId &&
      credential.principal === aliceId &&
      credential.kind === "oidc_subject" &&
      credential.issuer === "https://issuer.example" &&
      credential.subject === "00u123" &&
      credential.material_digest === "sha256:metadata",
  ),
);
loom.identityRevokeExternalCredential(authPath, externalId, null, rootId, "root-pass");
const revokedExternalIdentity = JSON.parse(loom.identityListJson(authPath, null, rootId, "root-pass"));
assert.ok(!revokedExternalIdentity.external_credentials.some((credential) => credential.id === externalId));
const publicKeyHex =
  "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296" +
  "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
const publicKeyId = loom.identityAddPublicKey(authPath, aliceId, "authority-laptop", "ES256", publicKeyHex, null, rootId, "root-pass");
const publicKeyIdentity = JSON.parse(loom.identityListJson(authPath, null, rootId, "root-pass"));
assert.ok(
  publicKeyIdentity.public_keys.some(
    (key) =>
      key.id === publicKeyId &&
      key.principal === aliceId &&
      key.label === "authority-laptop" &&
      key.algorithm === "ES256" &&
      key.public_key_hex === publicKeyHex,
  ),
);
loom.identityRevokePublicKey(authPath, publicKeyId, null, rootId, "root-pass");
const revokedPublicKeyIdentity = JSON.parse(loom.identityListJson(authPath, null, rootId, "root-pass"));
assert.ok(!revokedPublicKeyIdentity.public_keys.some((key) => key.id === publicKeyId));
loom.aclGrant(authPath, 0, aliceId, null, "files", 1, null, rootId, "root-pass");
loom.aclGrant(authPath, 0, `role:${adminRoleId}`, null, "kv", 1, null, rootId, "root-pass");
loom.aclGrantScoped(
  authPath,
  0,
  aliceId,
  null,
  "kv",
  3,
  "branch/main",
  ["key:tenant/a/", "key:tenant/b/"],
  null,
  rootId,
  "root-pass",
);
loom.aclGrantScoped(
  authPath,
  0,
  aliceId,
  null,
  "files",
  1,
  "branch/main",
  ["path:reports/"],
  null,
  rootId,
  "root-pass",
  "principal == 'alice'",
);
const grants = JSON.parse(loom.aclListJson(authPath, null, rootId, "root-pass"));
assert.ok(grants.some((g) => g.subject === aliceId && g.facet === "files" && g.rights.includes("read")));
assert.ok(grants.some((g) => g.subject === `role:${adminRoleId}` && g.subject_kind === "role" && g.facet === "kv"));
assert.ok(grants.some((g) => g.subject === aliceId && g.facet === "kv" && g.ref_glob === "branch/main" && g.scopes.length === 2));
assert.ok(
  grants.some(
    (g) =>
      g.subject === aliceId &&
      g.facet === "files" &&
      g.ref_glob === "branch/main" &&
      g.predicate?.language === "cel" &&
      g.predicate?.expression === "principal == 'alice'",
  ),
);
assert.equal(loom.aclRevoke(authPath, 0, aliceId, null, "files", 1, null, rootId, "root-pass"), true);
assert.equal(loom.aclRevoke(authPath, 0, aliceId, null, "files", 1, null, rootId, "root-pass"), false);
assert.equal(
  loom.aclRevokeScoped(
    authPath,
    0,
    aliceId,
    null,
    "files",
    1,
    "branch/main",
    ["path:reports/"],
    null,
    rootId,
    "root-pass",
    "principal == 'alice'",
  ),
  true,
);
assert.equal(
  loom.aclRevokeScoped(
    authPath,
    0,
    aliceId,
    null,
    "kv",
    3,
    "branch/main",
    ["key:tenant/a/", "key:tenant/b/"],
    null,
    rootId,
    "root-pass",
  ),
  true,
);
loom.protectedRefSet(
  authPath,
  "policy",
  "branch/main",
  true,
  false,
  false,
  0,
  true,
  false,
  null,
  rootId,
  "root-pass",
);
const protectedPolicy = JSON.parse(loom.protectedRefGetJson(authPath, "policy", "branch/main", null, rootId, "root-pass"));
assert.equal(protectedPolicy.fast_forward_only, true);
assert.equal(protectedPolicy.retention_lock, true);
const protectedPolicies = JSON.parse(loom.protectedRefListJson(authPath, "policy", null, rootId, "root-pass"));
assert.ok(protectedPolicies.some((policy) => policy.ref === "branch/main"));
assert.equal(loom.protectedRefRemove(authPath, "policy", "branch/main", null, rootId, "root-pass"), true);
assert.equal(loom.protectedRefGetJson(authPath, "policy", "branch/main", null, rootId, "root-pass"), "null");
assert.equal(loom.identityRevokeRole(authPath, aliceId, adminRoleId, null, rootId, "root-pass"), true);
assert.equal(loom.identityRevokeRole(authPath, aliceId, adminRoleId, null, rootId, "root-pass"), false);
const authSql = loom.LoomSql.authenticated(authPath, "authsql", "main", rootId, "root-pass");
authSql.exec("CREATE TABLE secured (id INTEGER PRIMARY KEY, v TEXT)");
authSql.exec("INSERT INTO secured VALUES (1, 'ok')");
assert.deepEqual(authSql.exec("SELECT v FROM secured WHERE id = 1")[0].rows, [["ok"]]);
const authBatch = loom.LoomSqlBatch.authenticated(authPath, "authbatch", "main", rootId, "root-pass");
authBatch.exec("CREATE TABLE secured (id INTEGER PRIMARY KEY, v TEXT)");
authBatch.exec("INSERT INTO secured VALUES (1, 'batch')");
authBatch.commit();
assert.deepEqual(authBatch.exec("SELECT v FROM secured WHERE id = 1")[0].rows, [["batch"]]);
authBatch.close();

// Removing the last passphrase recovery wrap while an external KEK remains needs the override.
const kpathRecovery = join(mkdtempSync(join(tmpdir(), "loom-")), "keys-recovery.loom");
loom.createLoom(kpathRecovery, "default", null, "recovery-pass");
loom.keyAddWrapWithKek(kpathRecovery, "recovery-pass", Buffer.alloc(32, 0x33), false);
assert.throws(() => loom.keyRemoveWrap(kpathRecovery, "recovery-pass", 0, false), /recovery|passphrase|external/i);

// Append-log queue: append assigns 0 then 1, len reflects appends, get/range round-trip.
const qpath2 = join(mkdtempSync(join(tmpdir(), "loom-")), "queue.loom");
loom.createLoom(qpath2, "default", null, null);
assert.equal(loom.queueAppend(qpath2, "events", "orders", Buffer.from("a")), 0n);
assert.equal(loom.queueAppend(qpath2, "events", "orders", Buffer.from("b")), 1n);
assert.equal(loom.queueAppend(qpath2, "events", "orders", Buffer.from("c")), 2n);
assert.equal(loom.queueLen(qpath2, "events", "orders"), 3n);
assert.equal(Buffer.from(loom.queueGet(qpath2, "events", "orders", 1n)).toString(), "b");
assert.equal(loom.queueGet(qpath2, "events", "orders", 9n), null);
const rangeOut = loom.queueRange(qpath2, "events", "orders", 1n, 3n).map((b) => Buffer.from(b).toString());
assert.deepEqual(rangeOut, ["b", "c"]);
assert.throws(() => loom.queueAppend(qpath2, "events", "../escape", Buffer.from("x")));

// Consumer offsets: missing reads as 0, read does not advance, advance is monotonic, reset moves back.
assert.equal(loom.queueConsumerPosition(qpath2, "events", "orders", "worker"), 0n);
const cread = loom.queueConsumerRead(qpath2, "events", "orders", "worker", 2).map((b) => Buffer.from(b).toString());
assert.deepEqual(cread, ["a", "b"]);
assert.equal(loom.queueConsumerPosition(qpath2, "events", "orders", "worker"), 0n);
loom.queueConsumerAdvance(qpath2, "events", "orders", "worker", 2n);
assert.equal(loom.queueConsumerPosition(qpath2, "events", "orders", "worker"), 2n);
assert.throws(() => loom.queueConsumerAdvance(qpath2, "events", "orders", "worker", 1n));
loom.queueConsumerReset(qpath2, "events", "orders", "worker", 0n);
assert.equal(loom.queueConsumerPosition(qpath2, "events", "orders", "worker"), 0n);
assert.throws(() => loom.queueConsumerPosition(qpath2, "events", "orders", "a/b"));

// Calendar facet: create a collection, write an entry via the iCalendar ingest path, list, delete.
const calPath = join(mkdtempSync(join(tmpdir(), "loom-")), "cal.loom");
loom.createLoom(calPath, "default", null, null);
loom.calCreateCollection(calPath, "ns", "alice", "work", "Work", "event");
const calEtag = loom.calPutIcs(
  calPath,
  "ns",
  "alice",
  "work",
  "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTART:20240101T090000Z\r\nSUMMARY:Hi\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
);
assert.match(calEtag, /^blake3:[0-9a-f]{64}$/, "calPutIcs returns an etag");
assert.ok(Buffer.from(loom.calListCollections(calPath, "ns", "alice")).length > 0, "calListCollections bytes");
assert.ok(Buffer.from(loom.calListEntries(calPath, "ns", "alice", "work")).length > 0, "calListEntries bytes");
assert.equal(loom.calDeleteCollection(calPath, "ns", "alice", "work"), true, "calDeleteCollection true");

// Contacts facet: create a book, write a vCard via the ingest path, list, delete.
const cardPath = join(mkdtempSync(join(tmpdir(), "loom-")), "card.loom");
loom.createLoom(cardPath, "default", null, null);
loom.cardCreateBook(cardPath, "ns", "alice", "main", "Main");
const cardEtag = loom.cardPutVcard(
  cardPath,
  "ns",
  "alice",
  "main",
  "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:c1\r\nFN:Jane\r\nEND:VCARD\r\n",
);
assert.match(cardEtag, /^blake3:[0-9a-f]{64}$/, "cardPutVcard returns an etag");
assert.ok(Buffer.from(loom.cardListBooks(cardPath, "ns", "alice")).length > 0, "cardListBooks bytes");
assert.equal(loom.cardDeleteBook(cardPath, "ns", "alice", "main"), true, "cardDeleteBook true");

// Mail facet: create a mailbox, ingest a message, body round-trips, index/list non-empty, flags + delete.
const mailPath = join(mkdtempSync(join(tmpdir(), "loom-")), "mail.loom");
loom.createLoom(mailPath, "default", null, null);
loom.mailCreateMailbox(mailPath, "ns", "alice", "inbox", "Inbox");
const rawMsg = Buffer.from("From: a@b\r\nSubject: Hi\r\n\r\nbody\r\n");
const mailEtag = loom.mailIngestMessage(mailPath, "ns", "alice", "inbox", "m1", rawMsg);
assert.match(mailEtag, /^blake3:[0-9a-f]{64}$/, "mailIngestMessage returns an etag");
assert.deepEqual(Buffer.from(loom.mailToEml(mailPath, "ns", "alice", "inbox", "m1")), rawMsg, "mailToEml round-trips");
assert.ok(loom.mailGetMessage(mailPath, "ns", "alice", "inbox", "m1") !== null, "mailGetMessage non-null");
assert.ok(Buffer.from(loom.mailListMessages(mailPath, "ns", "alice", "inbox")).length > 0, "mailListMessages bytes");
assert.ok(Buffer.isBuffer(Buffer.from(loom.mailGetFlags(mailPath, "ns", "alice", "inbox", "m1"))), "mailGetFlags bytes");
assert.equal(loom.mailDeleteMessage(mailPath, "ns", "alice", "inbox", "m1"), true, "mailDeleteMessage true");

// Workspace lifecycle: create with name + facet, list fields, rename by name and UUID, delete by UUID
// and name. Each call reopens the path, so a later read sees an earlier write.
const nspath = join(mkdtempSync(join(tmpdir(), "loom-")), "ns.loom");
loom.createLoom(nspath, "default", null, null);
const nsId = loom.workspaceCreate(nspath, "work", "files");
assert.match(nsId, /^[0-9a-f-]{36}$/);
const nsList = JSON.parse(loom.workspaceListJson(nspath));
assert.equal(nsList.length, 1);
assert.equal(nsList[0].id, nsId);
assert.equal(nsList[0].name, "work");
assert.deepEqual(nsList[0].facets, ["files"]);
assert.equal(nsList[0].head, null);
// Rename by name, then by UUID.
loom.workspaceRename(nspath, "work", "client");
assert.equal(JSON.parse(loom.workspaceListJson(nspath))[0].name, "client");
loom.workspaceRename(nspath, nsId, "client2");
assert.equal(JSON.parse(loom.workspaceListJson(nspath))[0].name, "client2");
// A missing workspace rename surfaces the engine error.
assert.throws(() => loom.workspaceRename(nspath, "missing", "x"));
// Delete the first by UUID and a second by name; deleted workspaces no longer appear.
loom.workspaceCreate(nspath, "second", "cas");
loom.workspaceDelete(nspath, nsId);
loom.workspaceDelete(nspath, "second");
assert.equal(loom.workspaceListJson(nspath), "[]");
assert.throws(() => loom.workspaceDelete(nspath, nsId));

// Direct table/history readers: sqlReadTable, sqlIndexScan (empty-array prefix matches all rows), sqlBlame,
// sqlDiff. Seed a table + index + rows through a SQL session, mirroring the C ABI direct-ops test, then
// decode each canonical-CBOR payload through resultToJson / resultToBridgeJson and assert structure.
const rpath = join(mkdtempSync(join(tmpdir(), "loom-")), "readers.loom");
const rdb = new loom.LoomSql(rpath, "app", "main");
rdb.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
rdb.exec("CREATE INDEX idx_v ON t (v)");
rdb.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')");
const c1 = rdb.commit("c1", "seed");
const tbl = ".loom/facets/sql/main/tables/t";

// sqlReadTable: a Rows envelope with the storage key plus user id/v columns.
const rt = JSON.parse(loom.resultToJson(loom.sqlReadTable(rpath, "app", tbl)));
assert.equal(rt.kind, "Rows", "sqlReadTable kind");
assert.deepEqual(rt.columns.map((c) => c.name), ["__key", "id", "v"], "sqlReadTable columns");
assert.deepEqual(
  rt.rows,
  [
    [{ Int: 1 }, { Int: 1 }, { Text: "a" }],
    [{ Int: 2 }, { Int: 2 }, { Text: "b" }],
  ],
  "sqlReadTable rows",
);
// resultToBridgeJson: lossless RN projection - text is bare, i64 is a tagged object.
const rtBridge = JSON.parse(loom.resultToBridgeJson(loom.sqlReadTable(rpath, "app", tbl)));
assert.equal(rtBridge.kind, "rows", "bridge kind");
assert.deepEqual(
  rtBridge.rows,
  [
    [{ $i64: "1" }, { $i64: "1" }, "a"],
    [{ $i64: "2" }, { $i64: "2" }, "b"],
  ],
  "bridge rows",
);

// sqlIndexScan with the canonical CBOR of an empty array (0x80) is the match-all lookup prefix.
const scan = JSON.parse(loom.resultToJson(loom.sqlIndexScan(rpath, "app", tbl, "idx_v", Buffer.from([0x80]))));
assert.equal(scan.kind, "Rows", "sqlIndexScan kind");
assert.deepEqual(scan.rows.map((r) => r[2]), [{ Text: "a" }, { Text: "b" }], "sqlIndexScan rows");

// sqlBlame: each current row plus the commit that last set it (all set by c1 here).
const blame = JSON.parse(loom.resultToJson(loom.sqlBlame(rpath, "app", "main", tbl)));
assert.equal(blame.kind, "Blame", "sqlBlame kind");
assert.equal(blame.rows.length, 2, "sqlBlame row count");
assert.ok(blame.rows.every((r) => r.commit === c1), "sqlBlame commits");
assert.deepEqual(blame.rows.map((r) => r.values[2]), [{ Text: "a" }, { Text: "b" }], "sqlBlame values");

// sqlDiff c1 -> c2: the third row is added.
rdb.exec("INSERT INTO t VALUES (3, 'c')");
const c2 = rdb.commit("c2", "seed");
const diff = JSON.parse(loom.resultToJson(loom.sqlDiff(rpath, "app", tbl, c1, c2)));
assert.equal(diff.kind, "Diff", "sqlDiff kind");
assert.deepEqual(
  diff.diffs,
  [{ change: "added", values: [{ Int: 3 }, { Int: 3 }, { Text: "c" }] }],
  "sqlDiff diffs",
);
const oldTable = JSON.parse(loom.resultToJson(loom.sqlReadTableAt(rpath, "app", tbl, c1)));
assert.deepEqual(oldTable.rows.map((r) => r[2]), [{ Text: "a" }, { Text: "b" }], "sqlReadTableAt rows");
const oldScan = JSON.parse(
  loom.resultToJson(loom.sqlIndexScanAt(rpath, "app", tbl, "idx_v", Buffer.from([0x80]), c1)),
);
assert.deepEqual(oldScan.rows.map((r) => r[2]), [{ Text: "a" }, { Text: "b" }], "sqlIndexScanAt rows");
const tableDiff = JSON.parse(loom.resultToJson(loom.sqlTableDiff(rpath, "app", tbl, c1, c2)));
assert.equal(tableDiff.kind, "TableDiff", "sqlTableDiff kind");
assert.deepEqual(
  tableDiff.records,
  [{ change: "added", values: [{ Int: 3 }, { Int: 3 }, { Text: "c" }] }],
  "sqlTableDiff rows",
);
rdb.exec("ALTER TABLE t ADD COLUMN n INTEGER DEFAULT 7");
const c3 = rdb.commit("c3", "seed");
const schemaDiff = JSON.parse(loom.resultToJson(loom.sqlTableDiff(rpath, "app", tbl, c2, c3)));
assert.equal(schemaDiff.records[0].change, "schema_changed", "sqlTableDiff schema change");
assert.equal(schemaDiff.records[0].to.columns.at(-1).name, "n", "sqlTableDiff schema column");

// Merge in-progress surface: on a fresh workspace nothing is in progress, conflicts are empty, and
// aborting with no merge throws. (The conflict happy-path is covered by the core and C ABI suites; the
// Node binding does not yet project merge/branch to create a conflict.)
const mpath = join(mkdtempSync(join(tmpdir(), "loom-")), "merge.loom");
loom.createLoom(mpath, "default", null, null);
loom.workspaceCreate(mpath, "work", "files");
assert.equal(loom.mergeInProgress(mpath, "files", "work"), false, "no merge in progress");
assert.deepEqual(loom.mergeConflicts(mpath, "files", "work"), [], "no conflicts");
assert.throws(() => loom.mergeAbort(mpath, "files", "work"), /no merge in progress/);

// Staging index over the SQL facet: an uncommitted change is unstaged; staging moves it to the shared
// index; commitStaged records only the index; status reports each transition.
const spath = join(mkdtempSync(join(tmpdir(), "loom-")), "staging.loom");
const sdb = new loom.LoomSql(spath, "app", "main");
sdb.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
sdb.exec("INSERT INTO t VALUES (1, 'a')");
sdb.commit("c1", "seed"); // commit everything: clean
const stbl = ".loom/facets/sql/main/tables/t";
sdb.exec("INSERT INTO t VALUES (2, 'b')"); // uncommitted working change
let sstat = JSON.parse(loom.statusJson(spath, "sql", "app"));
assert.ok(sstat.unstaged.some((c) => c.path === stbl), "modified table is unstaged");
loom.stage(spath, "sql", "app", [stbl]);
sstat = JSON.parse(loom.statusJson(spath, "sql", "app"));
assert.ok(
  sstat.staged.some((c) => c.path === stbl) && sstat.unstaged.length === 0,
  "table is staged",
);
const sc = loom.commitStaged(spath, "sql", "app", "seed", "staged insert");
assert.match(sc, /^blake3:[0-9a-f]{64}$/);
sstat = JSON.parse(loom.statusJson(spath, "sql", "app"));
assert.ok(sstat.staged.length === 0 && sstat.unstaged.length === 0, "clean after commitStaged");

// Files facade: write/read round-trip, append concatenation, remove.
const fpath = join(mkdtempSync(join(tmpdir(), "loom-")), "files.loom");
loom.createLoom(fpath, "default", null, null);
loom.workspaceCreate(fpath, "docs", "files");
loom.writeFile(fpath, "files", "docs", "a.txt", Buffer.from("hello"));
assert.equal(Buffer.from(loom.readFile(fpath, "files", "docs", "a.txt")).toString(), "hello");
loom.appendFile(fpath, "files", "docs", "a.txt", Buffer.from("!"));
assert.equal(Buffer.from(loom.readFile(fpath, "files", "docs", "a.txt")).toString(), "hello!");
loom.removeFile(fpath, "files", "docs", "a.txt");
assert.throws(() => loom.readFile(fpath, "files", "docs", "a.txt"));

// Symlink: create + read (git-style, opaque).
loom.symlink(fpath, "files", "docs", "some/target", "link");
assert.equal(loom.readLink(fpath, "files", "docs", "link"), "some/target");
assert.throws(() => loom.readLink(fpath, "files", "docs", "missing"));

// Restore: commit, edit, restore the path back from HEAD.
loom.writeFile(fpath, "files", "docs", "r.txt", Buffer.from("v1"));
loom.stageAll(fpath, "files", "docs");
loom.commitStaged(fpath, "files", "docs", "nas", "init");
loom.writeFile(fpath, "files", "docs", "r.txt", Buffer.from("v2"));
loom.restoreFile(fpath, "files", "docs", "HEAD", "r.txt");
assert.equal(Buffer.from(loom.readFile(fpath, "files", "docs", "r.txt")).toString(), "v1");
loom.restorePath(fpath, "files", "docs", "HEAD", "");

// Replay: commit a change, revert it (replayed JSON); empty cherry-pick and no-op rebase.
loom.writeFile(fpath, "files", "docs", "rep.txt", Buffer.from("x"));
loom.stageAll(fpath, "files", "docs");
const repCommit = loom.commitStaged(fpath, "files", "docs", "nas", "add rep");
const revOut = JSON.parse(loom.revert(fpath, "files", "docs", [repCommit], "nas", false));
assert.equal(revOut.outcome, "replayed");
assert.throws(() => loom.readFile(fpath, "files", "docs", "rep.txt"));
assert.equal(JSON.parse(loom.cherryPick(fpath, "files", "docs", [], false)).outcome, "empty");
assert.equal(JSON.parse(loom.rebase(fpath, "files", "docs", "HEAD", false)).outcome, "empty");

// Squash: two commits after a base collapse into one.
const sqBase = loom.commitStaged(fpath, "files", "docs", "nas", "sq base");
loom.writeFile(fpath, "files", "docs", "s1.txt", Buffer.from("1"));
loom.stageAll(fpath, "files", "docs");
loom.commitStaged(fpath, "files", "docs", "nas", "s1");
loom.writeFile(fpath, "files", "docs", "s2.txt", Buffer.from("2"));
loom.stageAll(fpath, "files", "docs");
loom.commitStaged(fpath, "files", "docs", "nas", "s2");
const squashed = loom.squash(fpath, "files", "docs", sqBase, "nas", "squashed");
assert.match(squashed, /^blake3:[0-9a-f]{64}$/);

// Byte-range I/O: write_at zero-fills the gap, read_at clamps, truncate shrinks.
loom.writeAt(fpath, "files", "docs", "b.bin", 5, Buffer.from("XY"));
assert.deepEqual(
  Array.from(loom.readAt(fpath, "files", "docs", "b.bin", 0, 100)),
  [0, 0, 0, 0, 0, 0x58, 0x59],
);
loom.truncateFile(fpath, "files", "docs", "b.bin", 6);
assert.deepEqual(
  Array.from(loom.readAt(fpath, "files", "docs", "b.bin", 0, 100)),
  [0, 0, 0, 0, 0, 0x58],
);

// File handle: open read-write, positional write, stat, read, close.
const fh = loom.fileOpen(fpath, "files", "docs", "b.bin", "read_write");
loom.fileWriteAt(fpath, fh, 0, Buffer.from("Z"));
const fstat = loom.fileStat(fpath, fh);
assert.equal(fstat.size, 6, "size after positional write");
assert.deepEqual(
  Array.from(loom.fileReadAt(fpath, fh, 0, 100)),
  [0x5a, 0, 0, 0, 0, 0x58],
);
loom.fileClose(fpath, fh);

// Tags: commit the files workspace, then create/list/target/rename/delete.
loom.stageAll(fpath, "files", "docs");
const tagCommit = loom.commitStaged(fpath, "files", "docs", "nas", "init");
assert.match(tagCommit, /^blake3:[0-9a-f]{64}$/);
// Lightweight tag at HEAD returns the commit digest.
assert.equal(loom.tagCreate(fpath, "files", "docs", "v1", "HEAD", "", ""), tagCommit);
// Annotated tag returns the tag object digest (not the commit).
const annTag = loom.tagCreate(fpath, "files", "docs", "v1-ann", "HEAD", "nas", "release 1");
assert.notEqual(annTag, tagCommit);
assert.deepEqual(loom.tagList(fpath, "files", "docs"), ["v1", "v1-ann"]);
assert.equal(loom.tagTarget(fpath, "files", "docs", "v1"), tagCommit);
loom.tagRename(fpath, "files", "docs", "v1", "v2");
assert.equal(loom.tagTarget(fpath, "files", "docs", "v2"), tagCommit);
loom.tagDelete(fpath, "files", "docs", "v2");
assert.equal(loom.tagTarget(fpath, "files", "docs", "v2"), null);
assert.throws(() => loom.tagDelete(fpath, "files", "docs", "v2"));

// Graph facade: nodes and a directed edge, neighbour/reachable traversal, edge removal.
const gpath = join(mkdtempSync(join(tmpdir(), "loom-")), "graph.loom");
loom.createLoom(gpath, "default", null, null);
const noProps = Buffer.alloc(0);
loom.graphUpsertNode(gpath, "graph", "g", "a", noProps);
loom.graphUpsertNode(gpath, "graph", "g", "b", noProps);
loom.graphUpsertEdge(gpath, "graph", "g", "e1", "a", "b", "rel", noProps);
assert.notEqual(loom.graphGetNode(gpath, "graph", "g", "a"), null);
assert.equal(loom.graphGetNode(gpath, "graph", "g", "zzz"), null);
// Canonical CBOR for ["b"]: 0x81 (array 1), 0x61 (text len 1), 0x62 ('b').
assert.deepEqual(Array.from(loom.graphNeighbors(gpath, "graph", "g", "a")), [0x81, 0x61, 0x62]);
assert.deepEqual(Array.from(loom.graphReachable(gpath, "graph", "g", "a", -1, "")), [0x81, 0x61, 0x62]);
assert.notEqual(loom.graphGetEdge(gpath, "graph", "g", "e1"), null);
assert.equal(loom.graphRemoveEdge(gpath, "graph", "g", "e1"), true);
assert.equal(loom.graphRemoveEdge(gpath, "graph", "g", "e1"), false);

// Vector facade: cosine set, upsert embeddings, exact search, delete.
const vpath = join(mkdtempSync(join(tmpdir(), "loom-")), "vector.loom");
loom.createLoom(vpath, "default", null, null);
const f32 = (x, y) => {
  const b = Buffer.alloc(8);
  b.writeFloatLE(x, 0);
  b.writeFloatLE(y, 4);
  return b;
};
loom.vectorCreate(vpath, "vec", "emb", 2n, 1);
loom.vectorUpsert(vpath, "vec", "emb", "a", f32(1.0, 0.0), Buffer.alloc(0));
loom.vectorUpsert(vpath, "vec", "emb", "c", f32(0.9, 0.1), Buffer.alloc(0));
assert.notEqual(loom.vectorGet(vpath, "vec", "emb", "a"), null);
assert.equal(loom.vectorGet(vpath, "vec", "emb", "zzz"), null);
const hits = Buffer.from(loom.vectorSearch(vpath, "vec", "emb", f32(1.0, 0.0), 2n, Buffer.alloc(0)));
assert.equal(hits[0], 0x82, "two hits -> CBOR array of 2");
assert.equal(loom.vectorDelete(vpath, "vec", "emb", "a"), true);
assert.equal(loom.vectorDelete(vpath, "vec", "emb", "a"), false);

// Columnar facade: typed columns, row append, count, predicate select.
const cpath = join(mkdtempSync(join(tmpdir(), "loom-")), "columnar.loom");
loom.createLoom(cpath, "default", null, null);
// columns [["id", 1 Int], ["price", 3 Text]] as canonical CBOR.
const cols = Buffer.from([0x82, 0x82, 0x62, 0x69, 0x64, 0x01, 0x82, 0x65, 0x70, 0x72, 0x69, 0x63, 0x65, 0x03]);
loom.columnarCreate(cpath, "col", "t", cols, 0n);
// rows: [Int(n), Text("n0")]; Int cell = [2, n], Text cell = [4, "x"].
loom.columnarAppend(cpath, "col", "t", Buffer.from([0x82, 0x82, 0x02, 0x01, 0x82, 0x04, 0x62, 0x31, 0x30]));
loom.columnarAppend(cpath, "col", "t", Buffer.from([0x82, 0x82, 0x02, 0x02, 0x82, 0x04, 0x62, 0x32, 0x30]));
assert.equal(loom.columnarRows(cpath, "col", "t"), 2n);
assert.equal(Buffer.from(loom.columnarScan(cpath, "col", "t"))[0], 0x82, "two rows scanned");
assert.equal(Buffer.from(loom.columnarInspect(cpath, "col", "t"))[0], 0x85, "inspect returns five fields");
assert.equal(Buffer.from(loom.columnarSourceDigest(cpath, "col", "t"))[0] & 0xe0, 0x60, "source digest is CBOR text");
// select ["price"] where id >= Int(2) (op 5 = ge): one matching row.
const selCols = Buffer.from([0x81, 0x65, 0x70, 0x72, 0x69, 0x63, 0x65]);
const selFilter = Buffer.from([0x83, 0x62, 0x69, 0x64, 0x05, 0x82, 0x02, 0x02]);
assert.equal(Buffer.from(loom.columnarSelect(cpath, "col", "t", selCols, selFilter))[0], 0x81, "one row selected");
const aggregates = Buffer.from([0x82, 0x82, 0x00, 0xf6, 0x82, 0x03, 0x62, 0x69, 0x64]);
assert.equal(Buffer.from(loom.columnarAggregate(cpath, "col", "t", aggregates, Buffer.alloc(0)))[0], 0x82, "two aggregate values");
loom.columnarCompact(cpath, "col", "t");
assert.equal(loom.columnarRows(cpath, "col", "t"), 2n);

// Search facade: a mapped collection, index/get/delete a document, and the linear-scan query.
const srpath = join(mkdtempSync(join(tmpdir(), "loom-")), "search.loom");
loom.createLoom(srpath, "default", null, null);
const srTitle = Buffer.from("title");
// mapping {"title": [0 text, true stored, false faceted]}.
const srMapping = Buffer.concat([Buffer.from([0xa1, 0x65]), srTitle, Buffer.from([0x83, 0x00, 0xf5, 0xf4])]);
loom.searchCreate(srpath, "search", "docs", srMapping);
// document {"title": "hello world"} (0x6b = text len 11).
const srDoc = Buffer.concat([Buffer.from([0xa1, 0x65]), srTitle, Buffer.from([0x6b]), Buffer.from("hello world")]);
loom.searchIndex(srpath, "search", "docs", Buffer.from("d1"), srDoc);
assert.notEqual(loom.searchGet(srpath, "search", "docs", Buffer.from("d1")), null);
assert.equal(loom.searchGet(srpath, "search", "docs", Buffer.from("zzz")), null);
// request [Match(title, "hello"), limit 10, offset 0].
const srQuery = Buffer.concat([Buffer.from([0x83, 0x00, 0x65]), srTitle, Buffer.from([0x65]), Buffer.from("hello")]);
const srRequest = Buffer.concat([Buffer.from([0x83]), srQuery, Buffer.from([0x0a, 0x00])]);
const srResp = Buffer.from(loom.searchQuery(srpath, "search", "docs", srRequest));
assert.equal(srResp[0], 0x84, "response is [reduced, hits, facets, aggregations]");
assert.equal(srResp[1], 0xf5, "the fallback marks reduced true");
assert.equal(loom.searchDelete(srpath, "search", "docs", Buffer.from("d1")), true);
assert.equal(loom.searchDelete(srpath, "search", "docs", Buffer.from("d1")), false);

console.log("node binding tests passed");
