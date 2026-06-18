@file:Suppress("DEPRECATION", "OVERRIDE_DEPRECATION")

package ai.uldren.loom.rn.host

import ai.uldren.loom.rn.UldrenLoomModule
import androidx.test.platform.app.InstrumentationRegistry
import com.facebook.react.bridge.BridgeReactContext
import com.facebook.react.bridge.JavaOnlyArray
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReadableArray
import com.facebook.react.bridge.ReadableMap
import com.facebook.react.bridge.WritableArray
import com.facebook.react.bridge.WritableMap
import com.facebook.react.soloader.OpenSourceMergedSoMapping
import com.facebook.soloader.SoLoader
import com.facebook.soloader.nativeloader.NativeLoader
import com.facebook.soloader.nativeloader.SystemDelegate
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class UldrenLoomHostRuntimeTest {
    @Test
    fun nativeModuleRoundTrip() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        SoLoader.init(context, OpenSourceMergedSoMapping)
        NativeLoader.initIfUninitialized(SystemDelegate())
        val module = UldrenLoomModule(BridgeReactContext(context))
        val loomPath = File(context.filesDir, "rn-${System.nanoTime()}.loom").absolutePath
        val empty = bytes()

        assertTrue(module.version().isNotBlank())
        assertTrue((await { module.runtimeProfile(it) } as ReadableArray).size() > 0)
        assertTrue(module.blobDigest(bytes(1, 2, 3)).isNotBlank())

        await { module.create(loomPath, "default", "", "", it) }
        val workspace = await { module.workspaceCreate(loomPath, "rn", "cas", "", empty, "", "", it) } as String
        val listed = JSONArray(await { module.workspaceListJson(loomPath, "", empty, "", "", it) } as String)
        assertEquals(workspace, listed.getJSONObject(0).getString("id"))
        assertEquals("rn", listed.getJSONObject(0).getString("name"))

        val digest = await { module.casPut(loomPath, "rn", bytes(10, 20, 30), "", empty, "", "", it) } as String
        assertEquals(true, await { module.casHas(loomPath, workspace, digest, "", empty, "", "", it) })
        assertEquals(listOf(10, 20, 30), toList(await { module.casGet(loomPath, "rn", digest, "", empty, "", "", it) } as ReadableArray))
        assertTrue((await { module.casListJson(loomPath, workspace, "", empty, "", "", it) } as String).contains(digest))

        assertEquals("0", await { module.queueAppend(loomPath, "queue", "events", bytes(7), "", empty, "", "", it) })
        assertEquals("1", await { module.queueLen(loomPath, "queue", "events", "", empty, "", "", it) })
        assertEquals(listOf(7), toList(await { module.queueGet(loomPath, "queue", "events", "0", "", empty, "", "", it) } as ReadableArray))
        assertEquals("0", await { module.queueConsumerPosition(loomPath, "queue", "events", "reader", "", empty, "", "", it) })
        await { module.queueConsumerAdvance(loomPath, "queue", "events", "reader", "1", "", empty, "", "", it) }
        assertEquals("1", await { module.queueConsumerPosition(loomPath, "queue", "events", "reader", "", empty, "", "", it) })

        await { module.sqlExecBytes(loomPath, "sql", "main", "CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)", "", empty, "", "", it) }
        await { module.sqlExecBytes(loomPath, "sql", "main", "CREATE INDEX idx_v ON t (v)", "", empty, "", "", it) }
        await { module.sqlExecBytes(loomPath, "sql", "main", "INSERT INTO t VALUES (1, 'a'), (2, 'b')", "", empty, "", "", it) }
        await { module.sqlExecBytes(loomPath, "sql", "main", "CREATE TABLE u (id INTEGER PRIMARY KEY, v TEXT)", "", empty, "", "", it) }
        await { module.sqlExecBytes(loomPath, "sql", "main", "INSERT INTO u VALUES (1, 'a')", "", empty, "", "", it) }
        val c1 = await { module.sqlCommit(loomPath, "sql", "main", "seed", "rn", "", empty, "", "", it) } as String
        await { module.sqlExecBytes(loomPath, "sql", "main", "INSERT INTO t VALUES (3, 'c')", "", empty, "", "", it) }
        val c2 = await { module.sqlCommit(loomPath, "sql", "main", "add row", "rn", "", empty, "", "", it) } as String
        val tablePath = ".loom/facets/sql/main/tables/t"
        val schemaPath = ".loom/facets/sql/main/tables/u"
        assertTrue((await { module.sqlReadTableAt(loomPath, "sql", tablePath, c1, "", empty, "", "", it) } as ReadableArray).size() > 0)
        assertTrue(
            (await { module.sqlIndexScanAt(loomPath, "sql", tablePath, "idx_v", bytes(0x80), c1, "", empty, "", "", it) } as ReadableArray)
                .size() > 0
        )
        assertTrue((await { module.sqlTableDiff(loomPath, "sql", tablePath, c1, c2, "", empty, "", "", it) } as ReadableArray).size() > 0)
        await { module.sqlExecBytes(loomPath, "sql", "main", "ALTER TABLE u ADD COLUMN n INTEGER DEFAULT 7", "", empty, "", "", it) }
        val c3 = await { module.sqlCommit(loomPath, "sql", "main", "add column", "rn", "", empty, "", "", it) } as String
        assertTrue((await { module.sqlTableDiff(loomPath, "sql", schemaPath, c2, c3, "", empty, "", "", it) } as ReadableArray).size() > 0)

        await { module.vectorCreate(loomPath, "vec", "emb", 2.0, 1.0, "", empty, "", "", it) }
        val point = bytes(0, 0, 0x80, 0x3f, 0, 0, 0, 0)
        val source = utf8("alpha source")
        await {
            module.vectorUpsertSource(
                loomPath, "vec", "emb", "a", point, empty, source, "test-embedding", "sha256:test",
                "", empty, "", "", it
            )
        }
        assertEquals(toList(source), toList(await { module.vectorSourceText(loomPath, "vec", "emb", "a", "", empty, "", "", it) } as ReadableArray))
        assertTrue(containsUtf8(await { module.vectorEmbeddingModel(loomPath, "vec", "emb", "", empty, "", "", it) } as ReadableArray, "test-embedding"))
        await { module.vectorUpsert(loomPath, "vec", "emb", "a", point, empty, "", empty, "", "", it) }
        assertEquals(null, await { module.vectorSourceText(loomPath, "vec", "emb", "a", "", empty, "", "", it) })
        assertEquals(
            listOf(0x80),
            toList(
                await {
                    module.vectorSearchPolicy(loomPath, "vec", "emb", point, 1.0, empty, 1.0, 0.0, 0.0, 1.0, 16.0, 8.0, "", empty, "", "", it)
                } as ReadableArray
            )
        )
        assertEquals(true, await { module.vectorCreateMetadataIndex(loomPath, "vec", "emb", "lang", "", empty, "", "", it) })
        assertEquals(false, await { module.vectorCreateMetadataIndex(loomPath, "vec", "emb", "lang", "", empty, "", "", it) })
        assertEquals(
            listOf(0x81, 0x64, 0x6c, 0x61, 0x6e, 0x67),
            toList(await { module.vectorMetadataIndexKeys(loomPath, "vec", "emb", "", empty, "", "", it) } as ReadableArray)
        )
        assertEquals(true, await { module.vectorDropMetadataIndex(loomPath, "vec", "emb", "lang", "", empty, "", "", it) })
        assertEquals(false, await { module.vectorDropMetadataIndex(loomPath, "vec", "emb", "lang", "", empty, "", "", it) })

        val renamed = "rn-renamed-${System.nanoTime()}"
        await { module.workspaceRename(loomPath, workspace, renamed, "", empty, "", "", it) }
        assertNotEquals("[]", await { module.workspaceListJson(loomPath, "", empty, "", "", it) })
        await { module.workspaceDelete(loomPath, renamed, "", empty, "", "", it) }

        val bootstrap = JSONObject(await { module.identityListJson(loomPath, "", empty, "", "", it) } as String)
        assertEquals(false, bootstrap.getBoolean("authenticated_mode"))
        val root = bootstrap.getString("root")
        await { module.identitySetPassphrase(loomPath, root, "root-pass", "", empty, "", "", it) }
        assertRejected { module.identityListJson(loomPath, "", empty, "", "", it) }
        await { module.authenticatePassphrase(loomPath, root, "root-pass", "", empty, it) }
        val alice = await { module.identityAddPrincipal(loomPath, "alice", "Alice", "user", "", empty, root, "root-pass", it) } as String
        await { module.identityRenamePrincipalHandle(loomPath, alice, "alice-renamed", "", empty, root, "root-pass", it) }
        await { module.identitySetPassphrase(loomPath, alice, "alice-pass", "", empty, root, "root-pass", it) }
        val identity = JSONObject(await { module.identityListJson(loomPath, "", empty, root, "root-pass", it) } as String)
        assertEquals(true, identity.getBoolean("authenticated_mode"))
        assertTrue(identity.toString().contains(alice))
        assertTrue(identity.toString().contains("alice-renamed"))
        val reader = roleId(identity, "reader")
        await { module.identityAssignRole(loomPath, alice, reader, "", empty, root, "root-pass", it) }
        val assigned = JSONObject(await { module.identityListJson(loomPath, "", empty, root, "root-pass", it) } as String)
        assertTrue(assigned.toString().contains(reader))
        assertEquals(true, await { module.identityRevokeRole(loomPath, alice, reader, "", empty, root, "root-pass", it) })
        assertEquals(false, await { module.identityRevokeRole(loomPath, alice, reader, "", empty, root, "root-pass", it) })
        await { module.aclGrant(loomPath, 0.0, alice, "", "files", 1.0, "", empty, root, "root-pass", it) }
        val grants = await { module.aclListJson(loomPath, "", empty, root, "root-pass", it) } as String
        assertTrue(grants.contains(alice))
        assertTrue(grants.contains("\"files\""))
        assertTrue(grants.contains("\"read\""))
        assertEquals(true, await { module.aclRevoke(loomPath, 0.0, alice, "", "files", 1.0, "", empty, root, "root-pass", it) })
        assertEquals(false, await { module.aclRevoke(loomPath, 0.0, alice, "", "files", 1.0, "", empty, root, "root-pass", it) })
        await { module.workspaceCreate(loomPath, "aclspace", "files", "", empty, root, "root-pass", it) }
        await {
            module.aclGrantScoped(
                loomPath, 0.0, alice, "aclspace", "files", 1.0, "branch/main",
                strings("path:public/"), "", empty, root, "root-pass", it
            )
        }
        val scopedGrants = await { module.aclListJson(loomPath, "", empty, root, "root-pass", it) } as String
        assertTrue(scopedGrants.contains("\"ref_glob\":\"branch/main\""))
        assertTrue(scopedGrants.contains("\"path\""))
        await {
            module.aclGrantScopedPredicate(
                loomPath, 0.0, alice, "aclspace", "files", 1.0, "branch/main",
                strings("path:reports/"), "principal == 'alice'", "", empty, root, "root-pass", it
            )
        }
        val predicateGrants = await { module.aclListJson(loomPath, "", empty, root, "root-pass", it) } as String
        assertTrue(predicateGrants.contains("\"language\":\"cel\""))
        assertTrue(predicateGrants.contains("principal == 'alice'"))
        assertEquals(
            true,
            await {
                module.aclRevokeScopedPredicate(
                    loomPath, 0.0, alice, "aclspace", "files", 1.0, "branch/main",
                    strings("path:reports/"), "principal == 'alice'", "", empty, root, "root-pass", it
                )
            }
        )
        assertEquals(
            true,
            await {
                module.aclRevokeScoped(
                    loomPath, 0.0, alice, "aclspace", "files", 1.0, "branch/main",
                    strings("path:public/"), "", empty, root, "root-pass", it
                )
            }
        )
        await {
            module.protectedRefSet(
                loomPath, "aclspace", "branch/main", true, false, false, 0.0, true, false,
                "", empty, root, "root-pass", it
            )
        }
        assertTrue(
            (await { module.protectedRefGetJson(loomPath, "aclspace", "branch/main", "", empty, root, "root-pass", it) } as String)
                .contains("\"retention_lock\":true")
        )
        assertTrue((await { module.protectedRefListJson(loomPath, "aclspace", "", empty, root, "root-pass", it) } as String).contains("\"ref\":\"branch/main\""))
        assertEquals(true, await { module.protectedRefRemove(loomPath, "aclspace", "branch/main", "", empty, root, "root-pass", it) })
        assertEquals("null", await { module.protectedRefGetJson(loomPath, "aclspace", "branch/main", "", empty, root, "root-pass", it) })

        val afterAuthDigest = await { module.casPut(loomPath, "rn", bytes(4, 5, 6), "", empty, root, "root-pass", it) } as String
        assertEquals(
            listOf(4, 5, 6),
            toList(await { module.casGet(loomPath, "rn", afterAuthDigest, "", empty, root, "root-pass", it) } as ReadableArray)
        )
        assertEquals("0", await { module.queueAppend(loomPath, "queue", "authorized", bytes(9), "", empty, root, "root-pass", it) })
        assertEquals("1", await { module.queueLen(loomPath, "queue", "authorized", "", empty, root, "root-pass", it) })
        val kvKey = bytes(0x82, 0x04, 0x61, 0x6b)
        await { module.kvPut(loomPath, "auth-kv", "items", kvKey, utf8("value"), "", empty, root, "root-pass", it) }
        assertEquals(
            toList(utf8("value")),
            toList(await { module.kvGet(loomPath, "auth-kv", "items", kvKey, "", empty, root, "root-pass", it) } as ReadableArray)
        )
        val textPut = await { module.docPutText(loomPath, "auth-docs", "notes", "a", "document", "", "", empty, root, "root-pass", it) } as ReadableMap
        val textDoc = await { module.docGetText(loomPath, "auth-docs", "notes", "a", "", empty, root, "root-pass", it) } as ReadableMap
        assertEquals("document", textDoc.getString("text"))
        assertEquals(textPut.getString("digest"), textDoc.getString("digest"))
        assertEquals(textPut.getString("entity_tag"), textDoc.getString("entity_tag"))
        val binaryPut = await { module.docPutBinary(loomPath, "auth-docs", "notes", "b", utf8("binary"), "", "", empty, root, "root-pass", it) } as ReadableMap
        val binaryDoc = await { module.docGetBinary(loomPath, "auth-docs", "notes", "b", "", empty, root, "root-pass", it) } as ReadableMap
        assertEquals(
            toList(utf8("binary")),
            toList(binaryDoc.getArray("bytes") as ReadableArray)
        )
        assertEquals(binaryPut.getString("digest"), binaryDoc.getString("digest"))
        assertEquals(binaryPut.getString("entity_tag"), binaryDoc.getString("entity_tag"))
        await { module.tsPut(loomPath, "auth-ts", "points", "1", utf8("point"), "", empty, root, "root-pass", it) }
        assertEquals(
            toList(utf8("point")),
            toList(await { module.tsGet(loomPath, "auth-ts", "points", "1", "", empty, root, "root-pass", it) } as ReadableArray)
        )
        assertEquals("0", await { module.ledgerAppend(loomPath, "auth-ledger", "entries", utf8("entry"), "", empty, root, "root-pass", it) })
        assertEquals(
            toList(utf8("entry")),
            toList(await { module.ledgerGet(loomPath, "auth-ledger", "entries", "0", "", empty, root, "root-pass", it) } as ReadableArray)
        )
        await { module.vectorCreate(loomPath, "auth-vec", "emb", 2.0, 1.0, "", empty, root, "root-pass", it) }
        await {
            module.vectorUpsertSource(
                loomPath, "auth-vec", "emb", "root-visible", point, empty, utf8("authorized vector"),
                "auth-test", "sha256:auth", "", empty, root, "root-pass", it
            )
        }
        assertEquals(
            toList(utf8("authorized vector")),
            toList(await { module.vectorSourceText(loomPath, "auth-vec", "emb", "root-visible", "", empty, root, "root-pass", it) } as ReadableArray)
        )
        val authWorkspace = await { module.workspaceCreate(loomPath, "after-auth", "files", "", empty, root, "root-pass", it) } as String
        assertTrue((await { module.workspaceListJson(loomPath, "", empty, root, "root-pass", it) } as String).contains(authWorkspace))
        await { module.workspaceDelete(loomPath, authWorkspace, "", empty, root, "root-pass", it) }
    }

    private fun bytes(vararg values: Int): WritableArray {
        val array = JavaOnlyArray()
        for (value in values) {
            array.pushInt(value)
        }
        return array
    }

    private fun strings(vararg values: String): WritableArray {
        val array = JavaOnlyArray()
        for (value in values) {
            array.pushString(value)
        }
        return array
    }

    private fun utf8(value: String): WritableArray =
        bytes(*value.encodeToByteArray().map { it.toInt() and 0xff }.toIntArray())

    private fun toList(array: ReadableArray): List<Int> {
        val values = ArrayList<Int>(array.size())
        for (i in 0 until array.size()) {
            values.add(array.getInt(i))
        }
        return values
    }

    private fun containsUtf8(array: ReadableArray, needle: String): Boolean {
        val values = toList(array)
        val target = needle.encodeToByteArray().map { it.toInt() and 0xff }
        return values.windowed(target.size).any { it == target }
    }

    private fun roleId(identity: JSONObject, name: String): String {
        val roles = identity.getJSONArray("roles")
        for (i in 0 until roles.length()) {
            val role = roles.getJSONObject(i)
            if (role.getString("name") == name) {
                return role.getString("id")
            }
        }
        throw AssertionError("missing role $name")
    }

    private fun await(call: (Promise) -> Unit): Any? {
        val promise = AwaitPromise()
        call(promise)
        return promise.await()
    }

    private fun assertRejected(call: (Promise) -> Unit) {
        try {
            await(call)
        } catch (_: AssertionError) {
            return
        }
        throw AssertionError("expected promise rejection")
    }

    private class AwaitPromise : Promise {
        private val latch = CountDownLatch(1)
        private var value: Any? = null
        private var error: Throwable? = null

        fun await(): Any? {
            check(latch.await(20, TimeUnit.SECONDS)) { "promise timed out" }
            error?.let { throw AssertionError("promise rejected", it) }
            return value
        }

        override fun resolve(value: Any?) {
            this.value = value
            latch.countDown()
        }

        override fun reject(code: String?, message: String?) = fail(code, message, null)
        override fun reject(code: String?, throwable: Throwable?) = fail(code, null, throwable)
        override fun reject(code: String?, message: String?, throwable: Throwable?) = fail(code, message, throwable)
        override fun reject(throwable: Throwable) = fail(null, null, throwable)
        override fun reject(throwable: Throwable, userInfo: WritableMap) = fail(null, null, throwable)
        override fun reject(code: String?, userInfo: WritableMap) = fail(code, null, null)
        override fun reject(code: String?, throwable: Throwable?, userInfo: WritableMap) = fail(code, null, throwable)
        override fun reject(code: String?, message: String?, userInfo: WritableMap) = fail(code, message, null)
        override fun reject(code: String?, message: String?, throwable: Throwable?, userInfo: WritableMap?) = fail(code, message, throwable)
        override fun reject(message: String) = fail(null, message, null)

        private fun fail(code: String?, message: String?, throwable: Throwable?) {
            val text = listOfNotNull(code, message).joinToString(": ").ifEmpty { "promise rejected" }
            error = RuntimeException(text, throwable)
            latch.countDown()
        }
    }
}
