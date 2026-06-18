import Foundation
import XCTest

@testable import UldrenLoom

final class LoomTests: XCTestCase {
    func testVersionIsNonEmpty() {
        XCTAssertFalse(Loom.version().isEmpty)
    }

    func testRuntimeProfileIsNonEmpty() throws {
        XCTAssertFalse(try Loom.runtimeProfile().isEmpty)
    }

    func testStudioSurfaceCatalogJson() throws {
        let data = Data(try Loom.studioSurfaceCatalogJson(workspace: "studio", set: "core").utf8)
        let value = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(value["workspace"] as? String, "studio")
        XCTAssertEqual(value["set"] as? String, "core")
        let apps = try XCTUnwrap(value["apps"] as? [[String: Any]])
        XCTAssertTrue(apps.contains { $0["app_id"] as? String == "ticket-details" })
        XCTAssertThrowsError(try Loom.studioSurfaceCatalogJson(workspace: "studio", set: "bogus"))
    }

    func testMeetingsImportSnapshotAndSourceRead() throws {
        let path = NSTemporaryDirectory() + UUID().uuidString + ".loom"
        try Loom.create(path: path, profile: "default")
        let store = try Loom.open(path: path)
        _ = try store.workspaceCreate(name: "studio", facet: "vcs")
        let snapshot: [String: Any] = [
            "snapshot_version": 1,
            "profile": "granola-app",
            "source_system": "granola-app",
            "source_scope": "local-cache",
            "observed_at": 500,
            "coverage": "complete",
            "items": [[
                "source_entity_id": "note-1",
                "source_digest": "blake3:" + String(repeating: "0", count: 64),
                "source_sidecar": ["id": "note-1", "raw": true],
                "title": "Planning",
                "summary_text": "Planning summary",
                "transcript_spans": [["text": "Capture decisions."]],
                "decisions": [["label": "Use normalized meeting imports."]],
            ]],
        ]
        let snapshotData = try JSONSerialization.data(withJSONObject: snapshot)
        let reportData = Data(try store.meetingsImportSnapshot(
            workspace: "studio",
            inputProfile: "granola-app",
            snapshot: Array(snapshotData)
        ).utf8)
        let report = try XCTUnwrap(JSONSerialization.jsonObject(with: reportData) as? [String: Any])
        XCTAssertEqual(report["profile"] as? String, "meetings")
        XCTAssertEqual(report["rows_imported"] as? Int, 1)
        let summary = try store.meetingsSourceRead(
            workspace: "studio",
            sourceId: "note-1",
            leaf: "summary.txt"
        )
        XCTAssertEqual(String(decoding: summary, as: UTF8.self), "Planning summary")
    }

    func testBlobDigestShape() {
        // The canonical "abc" vector lives in the `uldren-loom-conformance` crate; here we only
        // assert the address shape so the test needs no hard-coded digest: "blake3:" + 64 hex chars.
        let digest = Loom.blobDigest(Data("abc".utf8))
        XCTAssertTrue(digest.hasPrefix("blake3:"))
        XCTAssertEqual(digest.count, "blake3:".count + 64)
    }

    func testWatchSubscribePollRoundTrip() throws {
        let path = NSTemporaryDirectory() + UUID().uuidString + ".loom"
        try Loom.create(path: path, profile: "default")
        let store = try Loom.open(path: path)
        let db = try LoomSql(path: path, workspace: "watchapp", db: "main")
        _ = try db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        _ = try db.exec("INSERT INTO t VALUES (1, 'a')")
        let cursor = try store.watchSubscribe(workspace: "watchapp", branch: "main")
        XCTAssertTrue(try db.commit(message: "seed", author: "swift").hasPrefix("blake3:"))
        let batch = try store.watchPollBytes(cursor: cursor, max: 10)
        XCTAssertNotNil(Data(batch).range(of: Data("loom.watch.batch.v1".utf8)))
        XCTAssertNotNil(Data(batch).range(of: Data("unsupported_domains".utf8)))
        XCTAssertNotNil(Data(batch).range(of: Data("sql".utf8)))
    }

    // The shared cross-language fixture: bindings/ios/Tests/UldrenLoomTests/LoomTests.swift up four
    // directories is bindings/, then conformance/result-vectors.json.
    private func fixture() throws -> [String: Any] {
        var dir = URL(fileURLWithPath: #filePath)
        for _ in 0..<4 { dir.deleteLastPathComponent() }
        let url = dir.appendingPathComponent("conformance/result-vectors.json")
        let data = try Data(contentsOf: url)
        return try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    private func floats(_ values: [Float]) -> Data {
        var data = Data()
        data.reserveCapacity(values.count * MemoryLayout<Float>.size)
        for value in values {
            var bits = value.bitPattern.littleEndian
            withUnsafeBytes(of: &bits) { data.append(contentsOf: $0) }
        }
        return data
    }

    private func rootId(_ identityJson: String) throws -> String {
        let data = Data(identityJson.utf8)
        let value = try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [String: Any])
        return try XCTUnwrap(value["root"] as? String)
    }

    private func roleId(_ identityJson: String, name: String) throws -> String {
        let data = Data(identityJson.utf8)
        let value = try XCTUnwrap(
            JSONSerialization.jsonObject(with: data) as? [String: Any])
        let roles = try XCTUnwrap(value["roles"] as? [[String: Any]])
        let role = try XCTUnwrap(roles.first { $0["name"] as? String == name })
        return try XCTUnwrap(role["id"] as? String)
    }

    // Reproduce the shared exec vector through the Swift typed path and assert byte-for-byte equality
    // with the engine-pinned fixture - identical canonical CBOR means identical typed values across
    // every binding, since they all decode through the one shared Rust decoder.
    func testCrossLanguageResultVector() throws {
        let vectors = try XCTUnwrap(fixture()["vectors"] as? [String: Any])
        let vec = try XCTUnwrap(vectors["result_exec_select"] as? [String: Any])
        let sql = try XCTUnwrap(vec["sql"] as? [String])
        let execSql = try XCTUnwrap(vec["exec_sql"] as? String)
        let expectedHex = try XCTUnwrap(vec["canonical_hex"] as? String)

        let path = NSTemporaryDirectory() + UUID().uuidString + ".loom"
        let db = try LoomSql(path: path, workspace: "app", db: "main")
        _ = try db.exec(sql[0])  // CREATE TABLE t (id INTEGER PRIMARY KEY, n TEXT)
        _ = try db.exec(sql[1])  // INSERT INTO t VALUES (1, 'hi'), (2, NULL)

        // Raw canonical bytes must equal the fixture exactly.
        let bytes = try db.execBytes(execSql)
        let hex = bytes.map { String(format: "%02x", $0) }.joined()
        XCTAssertEqual(hex, expectedHex, "Swift execBytes drifted from the shared vector")

        // And the typed decode yields the same values: i64 1/2, text "hi", NULL.
        let result = try db.exec(execSql)
        XCTAssertTrue(result.isStatements)
        XCTAssertEqual(result.rowCount(0), 2)
        XCTAssertEqual(try result.cell(0, 0, 0).int64, 1)
        XCTAssertEqual(try result.cell(0, 0, 1).text, "hi")
        XCTAssertEqual(try result.cell(0, 1, 0).int64, 2)
        XCTAssertTrue(try result.cell(0, 1, 1).isNull)
    }

    // Direct table/history readers over the store session: sqlReadTable, sqlIndexScan (empty-array prefix
    // matches all rows), sqlBlame, and sqlDiff. Seed a table + index + rows through a SQL session,
    // mirroring the Node/Python direct-reader tests, then decode each typed LoomResult.
    func testDirectTableReaders() throws {
        let path = NSTemporaryDirectory() + UUID().uuidString + ".loom"
        let db = try LoomSql(path: path, workspace: "app", db: "main")
        _ = try db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)")
        _ = try db.exec("CREATE INDEX idx_v ON t (v)")
        _ = try db.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')")
        let c1 = try db.commit(message: "c1", author: "seed")
        let tbl = ".loom/facets/sql/main/tables/t"

        let store = try Loom.open(path: path)

        // sqlReadTable: storage key plus id/v columns and two typed rows.
        let rt = try store.sqlReadTable(workspace: "app", table: tbl)
        XCTAssertFalse(rt.isStatements)
        XCTAssertEqual(rt.columnCount(0), 3)
        XCTAssertEqual(try rt.columnName(0, 0), "__key")
        XCTAssertEqual(try rt.columnName(0, 1), "id")
        XCTAssertEqual(try rt.columnName(0, 2), "v")
        XCTAssertEqual(rt.rowCount(0), 2)
        XCTAssertEqual(try rt.cell(0, 0, 0).int64, 1)
        XCTAssertEqual(try rt.cell(0, 0, 1).int64, 1)
        XCTAssertEqual(try rt.cell(0, 0, 2).text, "a")
        XCTAssertEqual(try rt.cell(0, 1, 0).int64, 2)
        XCTAssertEqual(try rt.cell(0, 1, 1).int64, 2)
        XCTAssertEqual(try rt.cell(0, 1, 2).text, "b")

        // sqlIndexScan with the canonical CBOR of an empty array (0x80) is the match-all lookup prefix.
        let scan = try store.sqlIndexScan(
            workspace: "app", table: tbl, index: "idx_v", prefix: Data([0x80]))
        XCTAssertEqual(scan.rowCount(0), 2)
        XCTAssertEqual(try scan.cell(0, 0, 2).text, "a")
        XCTAssertEqual(try scan.cell(0, 1, 2).text, "b")

        // sqlBlame: each current row plus the commit that last set it (all set by c1 here).
        let blame = try store.sqlBlame(workspace: "app", branch: "main", table: tbl)
        XCTAssertEqual(blame.rowCount(0), 2)
        XCTAssertEqual(try blame.rowCommit(0, 0), c1)
        XCTAssertEqual(try blame.rowCommit(0, 1), c1)
        XCTAssertEqual(try blame.cell(0, 0, 2).text, "a")
        XCTAssertEqual(try blame.cell(0, 1, 2).text, "b")

        // sqlDiff c1 -> c2: the third row is added. Change kind LOOM_DIFF_ADDED = 0, read from side
        // LOOM_DIFF_SIDE_VALUES = 0.
        _ = try db.exec("INSERT INTO t VALUES (3, 'c')")
        let c2 = try db.commit(message: "c2", author: "seed")
        let diff = try store.sqlDiff(
            workspace: "app", table: tbl, fromCommit: c1, toCommit: c2)
        XCTAssertEqual(diff.diffCount(0), 1)
        XCTAssertEqual(try diff.diffChange(0, 0), 0)
        XCTAssertEqual(try diff.diffCell(0, 0, 0, 0).int64, 3)
        XCTAssertEqual(try diff.diffCell(0, 0, 0, 1).int64, 3)
        XCTAssertEqual(try diff.diffCell(0, 0, 0, 2).text, "c")

        let oldTable = try store.sqlReadTableAt(workspace: "app", table: tbl, commit: c1)
        XCTAssertEqual(oldTable.rowCount(0), 2)
        XCTAssertEqual(try oldTable.cell(0, 1, 2).text, "b")

        let oldScan = try store.sqlIndexScanAt(
            workspace: "app", table: tbl, index: "idx_v", prefix: Data([0x80]), commit: c1)
        XCTAssertEqual(oldScan.rowCount(0), 2)
        XCTAssertEqual(try oldScan.cell(0, 1, 2).text, "b")

        let tableDiffBytes = try store.sqlTableDiffBytes(
            workspace: "app", table: tbl, fromCommit: c1, toCommit: c2)
        XCTAssertFalse(tableDiffBytes.isEmpty)
    }

    func testVectorSourceModelRoundTrip() throws {
        let path = NSTemporaryDirectory() + UUID().uuidString + ".loom"
        try Loom.create(path: path, profile: "default")
        let store = try Loom.open(path: path)

        let point = floats([1.0, 0.0])
        let source = Data("alpha source".utf8)
        try store.vectorCreate(workspace: "vectors", name: "emb", dim: 2, metric: 1)
        try store.vectorUpsertSource(
            workspace: "vectors",
            name: "emb",
            id: "a",
            vector: point,
            sourceText: source,
            modelId: "test-embedding",
            weightsDigest: "sha256:test")

        XCTAssertEqual(try store.vectorSourceText(workspace: "vectors", name: "emb", id: "a"), source)
        let model = try XCTUnwrap(store.vectorEmbeddingModel(workspace: "vectors", name: "emb"))
        XCTAssertNotNil(model.range(of: Data("test-embedding".utf8)))

        try store.vectorUpsert(workspace: "vectors", name: "emb", id: "a", vector: point)
        XCTAssertNil(try store.vectorSourceText(workspace: "vectors", name: "emb", id: "a"))
    }

    func testIdentityAclRoundTrip() throws {
        let path = NSTemporaryDirectory() + UUID().uuidString + ".loom"
        try Loom.create(path: path, profile: "default")
        let store = try Loom.open(path: path)

        let bootstrap = try store.identityListJson()
        XCTAssertTrue(bootstrap.contains("\"authenticated_mode\":false"))
        let root = try rootId(bootstrap)
        _ = try store.workspaceCreate(name: "aclspace", facet: "files")

        try store.identitySetPassphrase(principal: root, passphrase: "root-pass")
        XCTAssertThrowsError(try store.identityListJson())
        try store.authenticatePassphrase(principal: root, passphrase: "root-pass")
        let alice = try store.identityAddPrincipal(handle: "alice", name: "Alice")
        try store.identitySetPassphrase(principal: alice, passphrase: "alice-pass")

        let listed = try store.identityListJson()
        XCTAssertTrue(listed.contains("\"authenticated_mode\":true"))
        XCTAssertTrue(listed.contains(alice))
        let reader = try roleId(listed, name: "reader")
        try store.identityAssignRole(principal: alice, role: reader)
        XCTAssertTrue(try store.identityListJson().contains(reader))
        XCTAssertTrue(try store.identityRevokeRole(principal: alice, role: reader))
        XCTAssertFalse(try store.identityRevokeRole(principal: alice, role: reader))

        try store.aclGrant(effect: 0, subject: alice, rightsMask: 1, domain: "files")
        let grants = try store.aclListJson()
        XCTAssertTrue(grants.contains(alice))
        XCTAssertTrue(grants.contains("\"files\""))
        XCTAssertTrue(grants.contains("\"read\""))
        XCTAssertTrue(try store.aclRevoke(effect: 0, subject: alice, rightsMask: 1, domain: "files"))
        XCTAssertFalse(try store.aclRevoke(effect: 0, subject: alice, rightsMask: 1, domain: "files"))

        try store.aclGrantScoped(
            effect: 0,
            subject: alice,
            rightsMask: 1,
            workspace: "aclspace",
            facet: "files",
            refGlob: "branch/main",
            scopes: [.path("public/")])
        let scopedGrants = try store.aclListJson()
        XCTAssertTrue(scopedGrants.contains("\"ref_glob\":\"branch/main\""))
        XCTAssertTrue(scopedGrants.contains("\"kind\":\"path\""))
        try store.aclGrantScopedPredicate(
            effect: 0,
            subject: alice,
            rightsMask: 1,
            workspace: "aclspace",
            facet: "files",
            refGlob: "branch/main",
            scopes: [.path("reports/")],
            predicateCel: "principal == 'alice'")
        let predicateGrants = try store.aclListJson()
        XCTAssertTrue(predicateGrants.contains("\"language\":\"cel\""))
        XCTAssertTrue(predicateGrants.contains("\"principal == 'alice'\""))
        XCTAssertTrue(try store.aclRevokeScopedPredicate(
            effect: 0,
            subject: alice,
            rightsMask: 1,
            workspace: "aclspace",
            facet: "files",
            refGlob: "branch/main",
            scopes: [.path("reports/")],
            predicateCel: "principal == 'alice'"))
        XCTAssertTrue(try store.aclRevokeScoped(
            effect: 0,
            subject: alice,
            rightsMask: 1,
            workspace: "aclspace",
            facet: "files",
            refGlob: "branch/main",
            scopes: [.path("public/")]))

        try store.protectedRefSet(
            workspace: "aclspace",
            refName: "branch/main",
            fastForwardOnly: true,
            signedCommitsRequired: false,
            signedRefAdvanceRequired: false,
            requiredReviewCount: 0,
            retentionLock: true,
            governanceLock: false)
        XCTAssertTrue(try store.protectedRefGetJson(
            workspace: "aclspace",
            refName: "branch/main").contains("\"retention_lock\":true"))
        XCTAssertTrue(try store.protectedRefListJson(workspace: "aclspace").contains("\"ref\":\"branch/main\""))
        XCTAssertTrue(try store.protectedRefRemove(workspace: "aclspace", refName: "branch/main"))
        XCTAssertEqual(try store.protectedRefGetJson(workspace: "aclspace", refName: "branch/main"), "null")
    }

    // CAS facet over the store session: put returns a content address (the raw content hash, distinct
    // from the Object::Blob digest) and is idempotent, has/get round-trip, list enumerates, and a
    // missing digest reads as absent.
    func testCasRoundTrip() throws {
        let path = NSTemporaryDirectory() + UUID().uuidString + ".loom"
        try Loom.create(path: path, profile: "default")
        let store = try Loom.open(path: path)

        let content = Data("hello loom".utf8)
        let addr = try store.casPut(workspace: "blobs", content: content)
        XCTAssertTrue(addr.hasPrefix("blake3:"))
        XCTAssertEqual(addr.count, "blake3:".count + 64)
        // Idempotent: identical bytes yield the same address.
        XCTAssertEqual(try store.casPut(workspace: "blobs", content: content), addr)
        XCTAssertTrue(try store.casHas(workspace: "blobs", digest: addr))
        XCTAssertEqual(try store.casGet(workspace: "blobs", digest: addr), content)

        // A digest that was never stored is absent.
        let missing = Loom.blobDigest(Data("never stored".utf8))
        XCTAssertFalse(try store.casHas(workspace: "blobs", digest: missing))
        XCTAssertNil(try store.casGet(workspace: "blobs", digest: missing))
        XCTAssertEqual(try store.casListJson(workspace: "blobs"), "[\"\(addr)\"]")
    }
}
