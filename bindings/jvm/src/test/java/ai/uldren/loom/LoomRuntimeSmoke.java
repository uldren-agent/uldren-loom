package ai.uldren.loom;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.List;

public final class LoomRuntimeSmoke {
    private LoomRuntimeSmoke() {
    }

    public static void main(String[] args) throws Exception {
        Path dir = Files.createTempDirectory("loom-jvm-runtime-");
        Path path = dir.resolve("runtime.loom");
        try {
            Loom.create(path.toString(), "default", null, null);
            assertTrue(!Loom.version().isBlank(), "version is blank");
            assertTrue(Loom.runtimeProfile().length > 0, "runtime profile is empty");
            String surfaceCatalog = Loom.studioSurfaceCatalogJson("studio", "core");
            assertContains(surfaceCatalog, "\"workspace\":\"studio\"", "surface catalog workspace");
            assertContains(surfaceCatalog, "\"app_id\":\"ticket-details\"", "surface catalog app");
            assertTrue(Loom.blobDigest(bytes("abc")).startsWith("blake3:"), "blob digest profile");

            try (LoomSession session = Loom.open(path.toString())) {
                verifyWorkspaces(session);
                verifyWatch(session);
                verifyCas(session);
                verifyMeetings(session);
                verifyChat(session);
                verifyQueue(session);
                verifyVector(session);
                verifySql(session);
                verifyIdentityAcl(session);
                verifyOrdinaryOpsAfterAuth(session);
            }
        } finally {
            Files.deleteIfExists(path);
            Files.deleteIfExists(dir);
        }
    }

    private static void verifyWorkspaces(LoomSession session) {
        String id = session.workspaces().create("work", "files");
        String listed = session.workspaces().listJson();
        assertContains(listed, id, "created workspace id");
        assertContains(listed, "\"work\"", "created workspace name");
        assertContains(listed, "\"files\"", "created workspace facet");

        session.workspaces().rename("work", "working");
        listed = session.workspaces().listJson();
        assertContains(listed, "\"working\"", "renamed workspace name");

        session.workspaces().delete(id);
        listed = session.workspaces().listJson();
        assertNotContains(listed, "\"working\"", "deleted workspace name");
    }

    private static void verifyWatch(LoomSession session) {
        try (Loom.LoomSql sql = session.sql("watchapp", "main")) {
            close(sql.exec("CREATE TABLE watch_t (id INTEGER PRIMARY KEY, v TEXT)"));
            close(sql.exec("INSERT INTO watch_t VALUES (1, 'a')"));
            String cursor = session.vcs().watchSubscribe("watchapp", "main", null, null, List.of(), null);
            assertTrue(sql.commit("seed", "jvm").startsWith("blake3:"), "watch sql commit");
            byte[] batch = session.vcs().watchPollBytes(cursor, 10);
            assertTrue(contains(batch, bytes("loom.watch.batch.v1")), "watch batch schema");
            assertTrue(contains(batch, bytes("unsupported_domains")), "watch unsupported domains");
            assertTrue(contains(batch, bytes("sql")), "watch sql domain");
        }
    }

    private static void verifyCas(LoomSession session) {
        byte[] content = bytes("hello");
        String digest = session.cas().put("blobs", content);
        assertEquals(digest, session.cas().put("blobs", content), "cas idempotent put");
        assertTrue(session.cas().has("blobs", digest), "cas has stored digest");
        assertBytes(content, session.cas().get("blobs", digest), "cas get");
        assertContains(session.cas().listJson("blobs"), digest, "cas list");
        assertTrue(session.cas().get("blobs", Loom.blobDigest(bytes("missing"))) == null, "cas missing get");
    }

    private static void verifyMeetings(LoomSession session) {
        session.workspaces().create("studio", "vcs");
        String snapshot = """
                {"snapshot_version":1,"profile":"granola-app","source_system":"granola-app",
                "source_scope":"local-cache","observed_at":500,"coverage":"complete","items":[{
                "source_entity_id":"note-1","source_digest":"blake3:0000000000000000000000000000000000000000000000000000000000000000",
                "source_sidecar":{"id":"note-1","raw":true},"title":"Planning",
                "summary_text":"Planning summary","transcript_spans":[{"text":"Capture decisions."}],
                "decisions":[{"label":"Use normalized meeting imports."}]}]}""";
        String report = session.meetings().importSnapshot("studio", "granola-app", bytes(snapshot), false);
        assertContains(report, "\"profile\":\"meetings\"", "meetings report profile");
        assertContains(report, "\"rows_imported\":1", "meetings rows imported");
        assertBytes(bytes("Planning summary"),
                session.meetings().sourceRead("studio", "note-1", "summary.txt"),
                "meetings retained summary");
    }

    private static void verifyChat(LoomSession session) {
        session.workspaces().create("chatspace", "vcs");
        ChatOps chat = session.chat();
        String chatWorkspaceId = "11111111-1111-1111-1111-111111111111";
        String channelId = "22222222-2222-2222-2222-222222222222";
        String channel = chat.createChannelJson("chatspace", chatWorkspaceId, channelId, "general", "General");
        assertContains(channel, "\"channel_id\":\"" + channelId + "\"", "chat channel id");
        assertContains(chat.listChannelsJson("chatspace", chatWorkspaceId), "\"general\"", "chat channel list");
        String posted = chat.postMessageJson("chatspace", chatWorkspaceId, channelId, "message-1", null, "hello");
        assertContains(posted, "\"operation_kind\":\"message.created\"", "chat post operation");
        assertContains(chat.messagesJson("chatspace", chatWorkspaceId, channelId), "\"message-1\"", "chat message list");
        chat.updateCursorJson("chatspace", chatWorkspaceId, channelId, 1);
        assertContains(chat.cursorJson("chatspace", chatWorkspaceId, channelId), "\"next_sequence\":1", "chat cursor");
        assertContains(chat.fetchEventsJson("chatspace", chatWorkspaceId, channelId, 1, 10), "\"events\"", "chat event fetch");
    }

    private static void verifyIdentityAcl(LoomSession session) {
        IdentityOps identity = session.identity();
        String bootstrap = identity.listJson();
        assertContains(bootstrap, "\"authenticated_mode\":false", "bootstrap auth mode");
        String root = rootId(bootstrap);
        session.workspaces().create("aclspace", "files");

        identity.setPassphrase(root, "root-pass");
        assertThrows(identity::listJson, "identity list before auth");
        identity.authenticatePassphrase(root, "root-pass");
        String alice = identity.addPrincipal("alice", "Alice", "user");
        identity.setPassphrase(alice, "alice-pass");

        String listed = identity.listJson();
        assertContains(listed, "\"authenticated_mode\":true", "authenticated mode");
        assertContains(listed, alice, "new principal");
        String reader = roleId(listed, "reader");
        identity.assignRole(alice, reader);
        assertContains(identity.listJson(), reader, "assigned reader role");
        assertTrue(identity.revokeRole(alice, reader), "role revoke");
        assertTrue(!identity.revokeRole(alice, reader), "role revoke absent");

        identity.aclGrant(0, alice, null, "files", 1);
        String grants = identity.aclListJson();
        assertContains(grants, alice, "acl subject");
        assertContains(grants, "\"files\"", "acl domain");
        assertContains(grants, "\"read\"", "acl right");
        assertTrue(identity.aclRevoke(0, alice, null, "files", 1), "acl revoke");
        assertTrue(!identity.aclRevoke(0, alice, null, "files", 1), "acl revoke absent");

        identity.aclGrantScoped(0, alice, "aclspace", "files", 1, "branch/main",
                new IdentityOps.AclScope[] { IdentityOps.AclScope.path("public/") });
        String scopedGrants = identity.aclListJson();
        assertContains(scopedGrants, "\"ref_glob\":\"branch/main\"", "scoped acl ref glob");
        assertContains(scopedGrants, "\"kind\":\"path\"", "scoped acl kind");
        identity.aclGrantScopedPredicate(0, alice, "aclspace", "files", 1, "branch/main",
                new IdentityOps.AclScope[] { IdentityOps.AclScope.path("reports/") },
                "principal == 'alice'");
        String predicateGrants = identity.aclListJson();
        assertContains(predicateGrants, "\"language\":\"cel\"", "predicate language");
        assertContains(predicateGrants, "principal == 'alice'", "predicate expression");
        assertTrue(identity.aclRevokeScopedPredicate(0, alice, "aclspace", "files", 1, "branch/main",
                new IdentityOps.AclScope[] { IdentityOps.AclScope.path("reports/") },
                "principal == 'alice'"), "predicate acl revoke");
        assertTrue(identity.aclRevokeScoped(0, alice, "aclspace", "files", 1, "branch/main",
                new IdentityOps.AclScope[] { IdentityOps.AclScope.path("public/") }), "scoped acl revoke");

        identity.protectedRefSet("aclspace", "branch/main", true, false, false, 0, true, false);
        assertContains(identity.protectedRefGetJson("aclspace", "branch/main"),
                "\"retention_lock\":true", "protected ref get");
        assertContains(identity.protectedRefListJson("aclspace"), "\"ref\":\"branch/main\"", "protected ref list");
        assertTrue(identity.protectedRefRemove("aclspace", "branch/main"), "protected ref remove");
        assertEquals("null", identity.protectedRefGetJson("aclspace", "branch/main"), "protected ref missing");
    }

    private static void verifyOrdinaryOpsAfterAuth(LoomSession session) {
        byte[] content = bytes("after-auth");
        String digest = session.cas().put("blobs", content);
        assertBytes(content, session.cas().get("blobs", digest), "cas after auth");
        session.queue().append("events", "authorized", bytes("visible"));
        assertEquals(1L, session.queue().len("events", "authorized"), "queue after auth");
        try (Loom.LoomSql sql = session.sql("secured_sql", "main")) {
            close(sql.exec("CREATE TABLE secured (id INTEGER PRIMARY KEY, v TEXT)"));
            close(sql.exec("INSERT INTO secured VALUES (1, 'ok')"));
            try (Loom.LoomResult result = sql.exec("SELECT v FROM secured WHERE id = 1")) {
                assertEquals("ok", result.cell(0, 0, 0).text(), "sql after auth");
            }
        }
    }

    private static void verifyQueue(LoomSession session) {
        byte[] first = bytes("one");
        byte[] second = bytes("two");
        assertEquals(0L, session.queue().append("events", "orders", first), "queue first seq");
        assertEquals(1L, session.queue().append("events", "orders", second), "queue second seq");
        assertEquals(2L, session.queue().len("events", "orders"), "queue len");
        assertBytes(first, session.queue().get("events", "orders", 0), "queue get first");
        assertTrue(session.queue().get("events", "orders", 9) == null, "queue missing get");
        assertTrue(session.queue().range("events", "orders", 0, 2).length > 0, "queue range cbor");
        assertEquals(0L, session.queue().consumerPosition("events", "orders", "worker"), "consumer initial");
        assertTrue(session.queue().consumerRead("events", "orders", "worker", 2).length > 0, "consumer read");
        session.queue().consumerAdvance("events", "orders", "worker", 2);
        assertEquals(2L, session.queue().consumerPosition("events", "orders", "worker"), "consumer advance");
        session.queue().consumerReset("events", "orders", "worker", 1);
        assertEquals(1L, session.queue().consumerPosition("events", "orders", "worker"), "consumer reset");
    }

    private static void verifyVector(LoomSession session) {
        VectorOps vector = session.vector();
        byte[] point = floats(1.0f, 0.0f);
        byte[] source = bytes("alpha source");
        vector.create("vectors", "emb", 2, 1);
        vector.upsertSource("vectors", "emb", "a", point, new byte[0], source, "test-embedding", "sha256:test");
        assertBytes(source, vector.sourceText("vectors", "emb", "a"), "vector source text");
        byte[] model = vector.embeddingModel("vectors", "emb");
        assertTrue(model != null && contains(model, bytes("test-embedding")), "vector embedding model");
        vector.upsert("vectors", "emb", "a", point, new byte[0]);
        assertTrue(vector.sourceText("vectors", "emb", "a") == null, "raw vector upsert clears source");
    }

    private static void verifySql(LoomSession session) {
        try (Loom.LoomSql sql = session.sql("app", "main")) {
            close(sql.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)"));
            close(sql.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')"));
            try (Loom.LoomResult result = sql.exec("SELECT id, v FROM t ORDER BY id")) {
                assertEquals(1L, result.len(), "sql item count");
                assertEquals(2L, result.columnCount(0), "sql column count");
                assertEquals("id", result.columnName(0, 0), "sql first column");
                assertEquals("v", result.columnName(0, 1), "sql second column");
                assertEquals(2L, result.rowCount(0), "sql row count");
                assertEquals(1L, result.cell(0, 0, 0).int64(), "sql first id");
                assertEquals("a", result.cell(0, 0, 1).text(), "sql first value");
                assertEquals(2L, result.cell(0, 1, 0).int64(), "sql second id");
                assertEquals("b", result.cell(0, 1, 1).text(), "sql second value");
            }
            assertTrue(sql.commit("seed", "jvm").startsWith("blake3:"), "sql commit digest");
        }
    }

    private static void close(Loom.LoomResult result) {
        result.close();
    }

    private static byte[] bytes(String value) {
        return value.getBytes(StandardCharsets.UTF_8);
    }

    private static byte[] floats(float... values) {
        ByteBuffer buffer = ByteBuffer.allocate(values.length * Float.BYTES).order(ByteOrder.LITTLE_ENDIAN);
        for (float value : values) {
            buffer.putFloat(value);
        }
        return buffer.array();
    }

    private static boolean contains(byte[] haystack, byte[] needle) {
        for (int i = 0; i <= haystack.length - needle.length; i++) {
            boolean matched = true;
            for (int j = 0; j < needle.length; j++) {
                if (haystack[i + j] != needle[j]) {
                    matched = false;
                    break;
                }
            }
            if (matched) {
                return true;
            }
        }
        return false;
    }

    private static String rootId(String identityJson) {
        String marker = "\"root\":\"";
        int start = identityJson.indexOf(marker);
        assertTrue(start >= 0, "identity root field");
        start += marker.length();
        int end = identityJson.indexOf('"', start);
        assertTrue(end > start, "identity root value");
        return identityJson.substring(start, end);
    }

    private static String roleId(String identityJson, String name) {
        String nameMarker = "\"name\":\"" + name + "\"";
        int namePos = identityJson.indexOf(nameMarker);
        assertTrue(namePos >= 0, "role name field");
        String marker = "\"id\":\"";
        int start = identityJson.lastIndexOf(marker, namePos);
        assertTrue(start >= 0, "role id field");
        start += marker.length();
        int end = identityJson.indexOf('"', start);
        assertTrue(end > start, "role id value");
        return identityJson.substring(start, end);
    }

    private static void assertThrows(Runnable op, String label) {
        try {
            op.run();
        } catch (RuntimeException expected) {
            return;
        }
        throw new AssertionError(label + ": expected failure");
    }

    private static void assertContains(String value, String expected, String label) {
        assertTrue(value.contains(expected), label + ": expected to contain " + expected + " in " + value);
    }

    private static void assertNotContains(String value, String unexpected, String label) {
        assertTrue(!value.contains(unexpected), label + ": expected not to contain " + unexpected + " in " + value);
    }

    private static void assertBytes(byte[] expected, byte[] actual, String label) {
        assertTrue(Arrays.equals(expected, actual), label + ": bytes differ");
    }

    private static void assertEquals(Object expected, Object actual, String label) {
        assertTrue(expected.equals(actual), label + ": expected " + expected + " but got " + actual);
    }

    private static void assertTrue(boolean condition, String label) {
        if (!condition) {
            throw new AssertionError(label);
        }
    }
}
