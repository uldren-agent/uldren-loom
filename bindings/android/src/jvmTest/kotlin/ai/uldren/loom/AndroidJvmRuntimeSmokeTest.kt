package ai.uldren.loom

import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertTrue
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.file.Files

class AndroidJvmRuntimeSmokeTest {
    @Test
    fun promotedSurfacesRoundTrip() {
        val dir = Files.createTempDirectory("loom-android-jvm-runtime-")
        val path = dir.resolve("runtime.loom")
        try {
            Loom.create(path.toString(), "default")
            assertTrue(Loom.version().isNotBlank())
            assertTrue(Loom.runtimeProfile().isNotEmpty())
            assertTrue(Loom.blobDigest("abc".bytes()).startsWith("blake3:"))

            val session = LoomSession.open(path.toString())
            verifyWorkspaces(session)
            verifyWatch(session)
            verifyCas(session)
            verifyQueue(session)
            verifySql(session)
            verifyVector(session)
            verifyIdentityAcl(session)
            verifyOrdinaryOpsAfterAuth(session)
        } finally {
            Files.deleteIfExists(path)
            Files.deleteIfExists(dir)
        }
    }

    private fun verifyWorkspaces(session: LoomSession) {
        val id = session.workspaces().create("work", "files")
        var listed = session.workspaces().listJson()
        assertTrue(listed.contains(id))
        assertTrue(listed.contains("\"work\""))
        assertTrue(listed.contains("\"files\""))

        session.workspaces().rename("work", "working")
        listed = session.workspaces().listJson()
        assertTrue(listed.contains("\"working\""))

        session.workspaces().delete(id)
        listed = session.workspaces().listJson()
        assertTrue(!listed.contains("\"working\""))
    }

    private fun verifyWatch(session: LoomSession) {
        val sql = session.sql("watchapp", "main")
        try {
            close(sql.exec("CREATE TABLE watch_t (id INTEGER PRIMARY KEY, v TEXT)"))
            close(sql.exec("INSERT INTO watch_t VALUES (1, 'a')"))
            val cursor = session.vcs().watchSubscribe("watchapp", "main")
            assertTrue(sql.commit("seed", "android-jvm").startsWith("blake3:"))
            val batch = session.vcs().watchPoll(cursor, 10u)
            assertTrue(batch.contains("loom.watch.batch.v1".bytes()))
            assertTrue(batch.contains("unsupported_domains".bytes()))
            assertTrue(batch.contains("sql".bytes()))
        } finally {
            sql.close()
        }
    }

    private fun verifyCas(session: LoomSession) {
        val content = "hello".bytes()
        val digest = session.cas().put("blobs", content)
        assertEquals(digest, session.cas().put("blobs", content))
        assertTrue(session.cas().has("blobs", digest))
        assertContentEquals(content, session.cas().get("blobs", digest))
        assertTrue(session.cas().listJson("blobs").contains(digest))
        assertEquals(null, session.cas().get("blobs", Loom.blobDigest("missing".bytes())))
    }

    private fun verifyIdentityAcl(session: LoomSession) {
        val identity = session.identity()
        val bootstrap = identity.listJson()
        assertTrue(bootstrap.contains("\"authenticated_mode\":false"))
        val root = rootId(bootstrap)
        session.workspaces().create("aclspace", "files")

        identity.setPassphrase(root, "root-pass")
        assertFails { identity.listJson() }
        identity.authenticatePassphrase(root, "root-pass")
        val alice = identity.addPrincipal("alice", "Alice", "user")
        identity.renamePrincipalHandle(alice, "alice-renamed")
        identity.setPassphrase(alice, "alice-pass")

        val listed = identity.listJson()
        assertTrue(listed.contains("\"authenticated_mode\":true"))
        assertTrue(listed.contains(alice))
        assertTrue(listed.contains("alice-renamed"))
        val reader = roleId(listed, "reader")
        identity.assignRole(alice, reader)
        assertTrue(identity.listJson().contains(reader))
        assertEquals(true, identity.revokeRole(alice, reader))
        assertEquals(false, identity.revokeRole(alice, reader))

        identity.aclGrant(0, alice, null, "files", 1)
        val grants = identity.aclListJson()
        assertTrue(grants.contains(alice))
        assertTrue(grants.contains("\"files\""))
        assertTrue(grants.contains("\"read\""))
        assertEquals(true, identity.aclRevoke(0, alice, null, "files", 1))
        assertEquals(false, identity.aclRevoke(0, alice, null, "files", 1))

        identity.aclGrantScoped(0, alice, "aclspace", "files", 1, "branch/main", listOf(AclScope.path("public/")))
        val scopedGrants = identity.aclListJson()
        assertTrue(scopedGrants.contains("\"ref_glob\":\"branch/main\""))
        assertTrue(scopedGrants.contains("\"kind\":\"path\""))
        identity.aclGrantScopedPredicate(
            0,
            alice,
            "aclspace",
            "files",
            1,
            "branch/main",
            listOf(AclScope.path("reports/")),
            "principal == 'alice'",
        )
        val predicateGrants = identity.aclListJson()
        assertTrue(predicateGrants.contains("\"language\":\"cel\""))
        assertTrue(predicateGrants.contains("principal == 'alice'"))
        assertEquals(
            true,
            identity.aclRevokeScopedPredicate(
                0,
                alice,
                "aclspace",
                "files",
                1,
                "branch/main",
                listOf(AclScope.path("reports/")),
                "principal == 'alice'",
            ),
        )
        assertEquals(
            true,
            identity.aclRevokeScoped(0, alice, "aclspace", "files", 1, "branch/main", listOf(AclScope.path("public/"))),
        )

        identity.protectedRefSet("aclspace", "branch/main", true, false, false, 0, true, false)
        assertTrue(identity.protectedRefGetJson("aclspace", "branch/main").contains("\"retention_lock\":true"))
        assertTrue(identity.protectedRefListJson("aclspace").contains("\"ref\":\"branch/main\""))
        assertEquals(true, identity.protectedRefRemove("aclspace", "branch/main"))
        assertEquals("null", identity.protectedRefGetJson("aclspace", "branch/main"))
    }

    private fun verifyOrdinaryOpsAfterAuth(session: LoomSession) {
        val content = "after-auth".bytes()
        val digest = session.cas().put("blobs", content)
        assertContentEquals(content, session.cas().get("blobs", digest))
        session.queue().append("events", "authorized", "visible".bytes())
        assertEquals(1L, session.queue().len("events", "authorized"))
        val key = byteArrayOf(0x82.toByte(), 0x04, 0x61, 0x6b)
        session.kv().put("authorized-kv", "items", key, "value".bytes())
        assertContentEquals("value".bytes(), session.kv().get("authorized-kv", "items", key))
        val textPut = session.document().putText("authorized-docs", "notes", "a", "document")
        val textDoc = session.document().getText("authorized-docs", "notes", "a")
        assertEquals("document", textDoc?.text)
        assertEquals(textPut.digest, textDoc?.digest)
        assertEquals(textPut.entityTag, textDoc?.entityTag)
        val binaryPut = session.document().putBinary("authorized-docs", "notes", "b", "binary".bytes())
        val binaryDoc = session.document().getBinary("authorized-docs", "notes", "b")
        assertContentEquals("binary".bytes(), binaryDoc?.bytes)
        assertEquals(binaryPut.digest, binaryDoc?.digest)
        assertEquals(binaryPut.entityTag, binaryDoc?.entityTag)
        session.timeSeries().put("authorized-ts", "points", 1, "point".bytes())
        assertContentEquals("point".bytes(), session.timeSeries().get("authorized-ts", "points", 1))
        assertEquals(0L, session.ledger().append("authorized-ledger", "entries", "entry".bytes()))
        assertContentEquals("entry".bytes(), session.ledger().get("authorized-ledger", "entries", 0))
        val vectors = session.vector()
        vectors.create("authorized-vectors", "emb", 2, 1)
        vectors.upsertSource(
            "authorized-vectors",
            "emb",
            "root-visible",
            floats(0.0f, 1.0f),
            byteArrayOf(),
            "authorized vector source".bytes(),
            "auth-test",
            "sha256:auth",
        )
        assertContentEquals(
            "authorized vector source".bytes(),
            vectors.sourceText("authorized-vectors", "emb", "root-visible"),
        )
        val id = session.workspaces().create("after-auth", "files")
        assertTrue(session.workspaces().listJson().contains(id))
        session.workspaces().delete(id)
    }

    private fun verifyQueue(session: LoomSession) {
        val first = "one".bytes()
        val second = "two".bytes()
        assertEquals(0L, session.queue().append("events", "orders", first))
        assertEquals(1L, session.queue().append("events", "orders", second))
        assertEquals(2L, session.queue().len("events", "orders"))
        assertContentEquals(first, session.queue().get("events", "orders", 0))
        assertEquals(null, session.queue().get("events", "orders", 9))
        assertTrue(session.queue().range("events", "orders", 0, 2).isNotEmpty())
        assertEquals(0L, session.queue().consumerPosition("events", "orders", "worker"))
        assertTrue(session.queue().consumerRead("events", "orders", "worker", 2).isNotEmpty())
        session.queue().consumerAdvance("events", "orders", "worker", 2)
        assertEquals(2L, session.queue().consumerPosition("events", "orders", "worker"))
        session.queue().consumerReset("events", "orders", "worker", 1)
        assertEquals(1L, session.queue().consumerPosition("events", "orders", "worker"))
    }

    private fun verifyVector(session: LoomSession) {
        val vectors = session.vector()
        val point = floats(1.0f, 0.0f)
        val source = "alpha source".bytes()
        vectors.create("vectors", "emb", 2, 1)
        vectors.upsertSource("vectors", "emb", "a", point, byteArrayOf(), source, "test-embedding", "sha256:test")
        assertContentEquals(source, vectors.sourceText("vectors", "emb", "a"))
        val model = assertNotNull(vectors.embeddingModel("vectors", "emb"))
        assertTrue(model.contains("test-embedding".bytes()))
        vectors.upsert("vectors", "emb", "a", point, byteArrayOf())
        assertEquals(null, vectors.sourceText("vectors", "emb", "a"))
    }

    private fun verifySql(session: LoomSession) {
        val sql = session.sql("app", "main")
        try {
            close(sql.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)"))
            close(sql.exec("CREATE INDEX idx_v ON t (v)"))
            close(sql.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')"))
            close(sql.exec("CREATE TABLE u (id INTEGER PRIMARY KEY, v TEXT)"))
            close(sql.exec("INSERT INTO u VALUES (1, 'a')"))
            val result = sql.exec("SELECT id, v FROM t ORDER BY id")
            try {
                assertEquals(1L, result.len())
                assertEquals(2L, result.columnCount(0))
                assertEquals("id", result.columnName(0, 0))
                assertEquals("v", result.columnName(0, 1))
                assertEquals(2L, result.rowCount(0))
                assertEquals(1L, result.cell(0, 0, 0).int64)
                assertEquals("a", result.cell(0, 0, 1).text())
                assertEquals(2L, result.cell(0, 1, 0).int64)
                assertEquals("b", result.cell(0, 1, 1).text())
            } finally {
                result.close()
            }
            val c1 = sql.commit("seed", "android-jvm")
            assertTrue(c1.startsWith("blake3:"))
            close(sql.exec("INSERT INTO t VALUES (3, 'c')"))
            val c2 = sql.commit("add row", "android-jvm")
            assertTrue(c2.startsWith("blake3:"))
            val tables = session.tables()
            val tablePath = ".loom/facets/sql/main/tables/t"
            val schemaPath = ".loom/facets/sql/main/tables/u"
            assertTrue(tables.readTableAt("app", tablePath, c1).isNotEmpty())
            assertTrue(tables.indexScanAt("app", tablePath, "idx_v", byteArrayOf(0x80.toByte()), c1).isNotEmpty())
            assertTrue(tables.tableDiff("app", tablePath, c1, c2).isNotEmpty())
            close(sql.exec("ALTER TABLE u ADD COLUMN n INTEGER DEFAULT 7"))
            val c3 = sql.commit("add column", "android-jvm")
            assertTrue(c3.startsWith("blake3:"))
            assertTrue(tables.tableDiff("app", schemaPath, c2, c3).isNotEmpty())
        } finally {
            sql.close()
        }
    }

    private fun close(result: LoomResult) {
        result.close()
    }

    private fun String.bytes(): ByteArray = encodeToByteArray()

    private fun floats(vararg values: Float): ByteArray {
        val buffer = ByteBuffer.allocate(values.size * 4).order(ByteOrder.LITTLE_ENDIAN)
        values.forEach(buffer::putFloat)
        return buffer.array()
    }

    private fun ByteArray.contains(needle: ByteArray): Boolean {
        if (needle.isEmpty()) return true
        for (i in 0..(size - needle.size)) {
            var matched = true
            for (j in needle.indices) {
                if (this[i + j] != needle[j]) {
                    matched = false
                    break
                }
            }
            if (matched) return true
        }
        return false
    }

    private fun rootId(identityJson: String): String {
        val marker = "\"root\":\""
        val start = identityJson.indexOf(marker)
        assertTrue(start >= 0)
        val valueStart = start + marker.length
        val end = identityJson.indexOf('"', valueStart)
        assertTrue(end > valueStart)
        return identityJson.substring(valueStart, end)
    }

    private fun roleId(identityJson: String, name: String): String {
        val nameMarker = "\"name\":\"$name\""
        val nameStart = identityJson.indexOf(nameMarker)
        assertTrue(nameStart >= 0)
        val marker = "\"id\":\""
        val start = identityJson.lastIndexOf(marker, nameStart)
        assertTrue(start >= 0)
        val valueStart = start + marker.length
        val end = identityJson.indexOf('"', valueStart)
        assertTrue(end > valueStart)
        return identityJson.substring(valueStart, end)
    }

    private fun assertFails(block: () -> Unit) {
        try {
            block()
        } catch (_: RuntimeException) {
            return
        }
        throw AssertionError("expected failure")
    }
}
