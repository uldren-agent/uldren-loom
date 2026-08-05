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
        if (args.length == 1 && args[0].equals("operational")) {
            runOperationalOnly();
            return;
        }
        if (args.length == 1 && args[0].equals("interchange")) {
            runInterchangeOnly();
            return;
        }
        if (args.length == 1 && args[0].equals("data-execution")) {
            runDataExecutionOnly();
            return;
        }
        if (args.length == 1 && args[0].equals("drive")) {
            runDriveOnly();
            return;
        }
        if (args.length == 1 && args[0].equals("chat")) {
            runChatOnly();
            return;
        }
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
                verifyOperational(session);
            }
            try (LoomSession reopened = Loom.open(path.toString())) {
                verifyOperationalPersistence(reopened);
            }
            try (LoomSession session = Loom.open(path.toString())) {
                verifyInterchange(session);
            }
            try (LoomSession reopened = Loom.open(path.toString())) {
                verifyInterchangePersistence(reopened);
            }
        } finally {
            Files.deleteIfExists(path);
            Files.deleteIfExists(dir);
        }
    }

    private static void runOperationalOnly() throws Exception {
        Path dir = Files.createTempDirectory("loom-jvm-operational-runtime-");
        Path path = dir.resolve("runtime.loom");
        try {
            Loom.create(path.toString(), "default", null, null);
            try (LoomSession session = Loom.open(path.toString())) {
                verifyOperational(session);
            }
            try (LoomSession reopened = Loom.open(path.toString())) {
                verifyOperationalPersistence(reopened);
            }
        } finally {
            Files.deleteIfExists(path);
            Files.deleteIfExists(dir);
        }
    }

    private static void runInterchangeOnly() throws Exception {
        Path dir = Files.createTempDirectory("loom-jvm-interchange-runtime-");
        Path path = dir.resolve("runtime.loom");
        try {
            Loom.create(path.toString(), "default", null, null);
            try (LoomSession session = Loom.open(path.toString())) {
                verifyInterchange(session);
            }
            try (LoomSession reopened = Loom.open(path.toString())) {
                verifyInterchangePersistence(reopened);
            }
        } finally {
            Files.deleteIfExists(path);
            Files.deleteIfExists(dir);
        }
    }

    private static void runDataExecutionOnly() throws Exception {
        Path dir = Files.createTempDirectory("loom-jvm-data-execution-runtime-");
        Path path = dir.resolve("runtime.loom");
        try {
            Loom.create(path.toString(), "default", null, null);
            try (LoomSession session = Loom.open(path.toString())) {
                verifyDataExecution(session);
            }
            try (LoomSession reopened = Loom.open(path.toString())) {
                verifyDataExecutionPersistence(reopened);
            }
        } finally {
            Files.deleteIfExists(path);
            Files.deleteIfExists(dir);
        }
    }

    private static void runDriveOnly() throws Exception {
        Path dir = Files.createTempDirectory("loom-jvm-drive-runtime-");
        Path path = dir.resolve("runtime.loom");
        try {
            Loom.create(path.toString(), "default", null, null);
            try (LoomSession session = Loom.open(path.toString())) {
                verifyDriveReadHierarchy(session);
            }
            try (LoomSession reopened = Loom.open(path.toString())) {
                verifyDriveReadHierarchyPersistence(reopened);
            }
        } finally {
            Files.deleteIfExists(path);
            Files.deleteIfExists(dir);
        }
    }

    private static void runChatOnly() throws Exception {
        Path dir = Files.createTempDirectory("loom-jvm-chat-runtime-");
        Path path = dir.resolve("runtime.loom");
        try {
            Loom.create(path.toString(), "default", null, null);
            try (LoomSession session = Loom.open(path.toString())) {
                verifyChatChannelMessage(session);
                verifyChatTaskAgent(session);
            }
            try (LoomSession reopened = Loom.open(path.toString())) {
                ChatOps chat = reopened.chat();
                String chatWorkspaceId = "11111111-1111-1111-1111-111111111111";
                String channelId = "22222222-2222-2222-2222-222222222222";
                assertContains(chat.messagesJson("chatspace", chatWorkspaceId, channelId),
                        "\"message-bytes\"", "chat byte message persistence");
                assertContains(chat.messagesJson("chatspace", chatWorkspaceId, channelId),
                        "\"task-1\"", "chat task persistence");
                assertContains(chat.messagesJson("chatspace", chatWorkspaceId, channelId),
                        "\"inv-bytes\"", "chat byte prompt invocation persistence");
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
        verifyChatChannelMessage(session);
        ChatOps chat = session.chat();
        String chatWorkspaceId = "11111111-1111-1111-1111-111111111111";
        String channelId = "22222222-2222-2222-2222-222222222222";
        String messageTag = jsonStringField(chat.postMessageJson("chatspace", chatWorkspaceId,
                channelId, "reaction-source", null, "react here", null), "entity_tag");
        String reaction = chat.addReactionJson("chatspace", chatWorkspaceId, channelId,
                "reaction-source", "thumbs-up", null);
        assertContains(reaction, "\"operation_kind\":\"reaction.added\"", "chat reaction add");
        assertThrows(() -> chat.removeReactionJson("chatspace", chatWorkspaceId, channelId,
                "reaction-source", "thumbs-up", messageTag), "chat reaction stale entity tag");
        String emoji = chat.emojiRegisterJson("chatspace", chatWorkspaceId, "party", null);
        assertContains(emoji, "\"operation_kind\":\"emoji.registered\"", "chat emoji register");
        assertContains(chat.emojiListJson("chatspace", chatWorkspaceId), "\"party\"", "chat emoji list");
        String emojiRemoved = chat.emojiUnregisterJson("chatspace", chatWorkspaceId, "party", null);
        assertContains(emojiRemoved, "\"operation_kind\":\"emoji.unregistered\"", "chat emoji unregister");
        String cursorTag = jsonStringField(chat.cursorJson("chatspace", chatWorkspaceId, channelId),
                "entity_tag");
        chat.updateCursorJson("chatspace", chatWorkspaceId, channelId, 1, null);
        assertThrows(() -> chat.updateCursorJson("chatspace", chatWorkspaceId, channelId, 2,
                cursorTag), "chat cursor stale entity tag");
        assertContains(chat.cursorJson("chatspace", chatWorkspaceId, channelId), "\"next_sequence\":1", "chat cursor");
        assertContains(chat.fetchEventsJson("chatspace", chatWorkspaceId, channelId, 1, 10), "\"events\"", "chat event fetch");
    }

    private static void verifyChatChannelMessage(LoomSession session) {
        session.workspaces().create("chatspace", "vcs");
        ChatOps chat = session.chat();
        String chatWorkspaceId = "11111111-1111-1111-1111-111111111111";
        String channelId = "22222222-2222-2222-2222-222222222222";
        String channel = chat.createChannelJson("chatspace", chatWorkspaceId, channelId, "general", "General", null);
        assertContains(channel, "\"channel_id\":\"" + channelId + "\"", "chat channel id");
        assertContains(chat.listChannelsJson("chatspace", chatWorkspaceId), "\"general\"", "chat channel list");
        assertThrows(() -> chat.renameChannelJson("chatspace", chatWorkspaceId, channelId,
                "blocked", "not-current"), "chat expected entity tag mismatch");
        String posted = chat.postMessageJson("chatspace", chatWorkspaceId, channelId, "message-1", null, "hello", null);
        assertContains(posted, "\"operation_kind\":\"message.created\"", "chat post operation");
        String bytesPosted = chat.postMessageBytesJson("chatspace", chatWorkspaceId, channelId,
                "message-bytes", null, bytes("hello bytes"), null);
        assertContains(bytesPosted, "\"operation_kind\":\"message.created\"", "chat post bytes operation");
        String bytesEdited = chat.editMessageBytesJson("chatspace", chatWorkspaceId, channelId,
                "message-bytes", bytes("edited bytes"), null);
        assertContains(bytesEdited, "\"operation_kind\":\"message.edited\"", "chat edit bytes operation");
        assertContains(chat.messagesJson("chatspace", chatWorkspaceId, channelId), "\"message-1\"", "chat message list");
        assertContains(chat.messagesJson("chatspace", chatWorkspaceId, channelId), "\"message-bytes\"",
                "chat byte message list");
    }

    private static void verifyChatTaskAgent(LoomSession session) {
        ChatOps chat = session.chat();
        String chatWorkspaceId = "11111111-1111-1111-1111-111111111111";
        String channelId = "22222222-2222-2222-2222-222222222222";
        String agent = "33333333-3333-4333-8333-333333333333";
        String recipient = "44444444-4444-4444-8444-444444444444";
        String sourceTag = jsonStringField(chat.postMessageJson("chatspace", chatWorkspaceId,
                channelId, "task-source", null, "task source", null), "entity_tag");
        String task = chat.createTaskJson("chatspace", chatWorkspaceId, channelId, "task-1",
                "task-source", "Do it", null);
        assertContains(task, "\"operation_kind\":\"task.created\"", "chat task create");
        String claim = chat.claimTaskJson("chatspace", chatWorkspaceId, channelId, "task-1",
                "claim-1", "lease-1", null);
        assertContains(claim, "\"operation_kind\":\"task.claimed\"", "chat task claim");
        chat.postMessageJson("chatspace", chatWorkspaceId, channelId, "task-result", null, "done", null);
        String complete = chat.completeTaskJson("chatspace", chatWorkspaceId, channelId, "task-1",
                "claim-1", "task-result", null);
        assertContains(complete, "\"operation_kind\":\"task.completed\"", "chat task complete");
        String invoked = chat.invokeAgentJson("chatspace", chatWorkspaceId, channelId,
                "inv-text", agent, "[\"task-source\"]", "prompt", null);
        assertContains(invoked, "\"operation_kind\":\"agent.invoked\"", "chat agent invoke");
        String replied = chat.agentReplyJson("chatspace", chatWorkspaceId, channelId,
                "inv-text", "task-result", null);
        assertContains(replied, "\"operation_kind\":\"agent.replied\"", "chat agent reply");
        byte[] prompt = new byte[] { 0, (byte) 0xff, 0x61, (byte) 0xfe, 0x62 };
        String byteInvoke = chat.invokeAgentBytesJson("chatspace", chatWorkspaceId, channelId,
                "inv-bytes", agent, "[\"task-source\"]", prompt, null);
        assertContains(byteInvoke, "\"operation_kind\":\"agent.invoked\"", "chat agent byte invoke");
        assertThrows(() -> chat.invokeAgentBytesJson("chatspace", chatWorkspaceId, channelId,
                "inv-stale", agent, "[\"task-source\"]", bytes("stale"), sourceTag),
                "chat byte invoke stale entity tag");
        String handoffAbsent = chat.requestHandoffJson("chatspace", chatWorkspaceId, channelId,
                "handoff-absent", agent, null, null, null);
        assertContains(handoffAbsent, "\"operation_kind\":\"handoff.requested\"", "chat handoff absent");
        String handoffPresent = chat.requestHandoffJson("chatspace", chatWorkspaceId, channelId,
                "handoff-present", agent, recipient, "please take it", null);
        assertContains(handoffPresent, "\"operation_kind\":\"handoff.requested\"", "chat handoff present");
        String messages = chat.messagesJson("chatspace", chatWorkspaceId, channelId);
        assertContains(messages, "\"result_message_id\":\"task-result\"", "chat task result");
        assertContains(messages, "\"prompt\":[0,255,97,254,98]", "chat byte prompt");
        assertContains(messages, "\"to_principal\":null", "chat handoff absent principal");
        assertContains(messages, "\"to_principal\":\"" + recipient + "\"", "chat handoff recipient");
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

    private static void verifyOperational(LoomSession session) {
        session.workspaces().create("ops-studio", "vcs");
        session.workspaces().create("ops-ai", "inference");
        session.workspaces().create("ops-site", "vcs");
        String snapshot = """
                {"snapshot_version":1,"profile":"granola-app","source_system":"granola-app",
                "source_scope":"local-cache","observed_at":600,"coverage":"complete","items":[{
                "source_entity_id":"ops-meeting-1","source_digest":"blake3:0000000000000000000000000000000000000000000000000000000000000000",
                "source_sidecar":{"id":"ops-meeting-1"},"title":"Operational",
                "summary_text":"Operational summary","transcript_spans":[{"text":"Verify bindings."}],
                "decisions":[{"label":"Use generated operational wrappers."}]}]}""";
        session.meetings().importSnapshot("ops-studio", "granola-app", bytes(snapshot), false);

        StudioMaintenanceOps studio = session.studioMaintenance();
        String reindex = studio.reindexJson("ops-studio", "pages");
        assertContains(reindex, "\"profile\":\"pages\"", "studio reindex profile");
        String rebuild = studio.revisionsRebuildJson("ops-studio", "meetings", true);
        assertContains(rebuild, "\"profile\":\"meetings\"", "studio revisions rebuild profile");

        byte[] importReport = session.storeAdmin().bundleImport(minimalBundle(), false);
        assertTrue(contains(importReport, bytes("bundle-src")), "store bundle import workspace");
        assertContains(session.workspaces().listJson(), "\"bundle-src\"", "imported bundle workspace");

        InferenceInstanceOps inference = session.inferenceInstance();
        String created = inference.createJson("ops-ai", "embed",
                "sentence-transformers/all-MiniLM-L6-v2", "text-embedding", "hosted-api",
                "fast", "{\"batch_size\":\"4\"}");
        assertContains(created, "\"name\":\"embed\"", "inference create name");
        assertContains(created, "\"preset\":\"fast\"", "inference create preset");
        String updated = inference.updateJson("ops-ai", "embed", "deterministic",
                "{\"batch_size\":\"8\"}");
        assertContains(updated, "\"preset\":\"deterministic\"", "inference update preset");
        String deleted = inference.deleteJson("ops-ai", "embed");
        assertContains(deleted, "\"name\":\"embed\"", "inference delete name");

        ServeConfigOps serve = session.serveConfig();
        String listener = serve.listenerConfigureJson("""
                {"surface":"web","selectors":["ops-site"],"bind":"127.0.0.1:19180",
                "transport":"rest","enabled":true}""");
        String listenerId = jsonStringField(listener, "id");
        assertContains(listener, "\"surface\":\"web\"", "serve listener surface");
        assertContains(serve.listenerListJson(), listenerId, "serve listener list");
        String routeTable = serve.webRouteSetJson("""
                {"listener":"%s","route":"docs","prefix":"/docs","workspace":"ops-site",
                "root":"/docs"}""".formatted(listenerId));
        assertContains(routeTable, "\"route_id\":\"docs\"", "serve route set id");
        assertContains(serve.webRouteListJson(listenerId), "\"route_id\":\"docs\"", "serve route list");
        assertContains(serve.listenerSetEnabledJson(listenerId, false), "\"enabled\":false",
                "serve listener disable");
        assertContains(serve.listenerSetEnabledJson(listenerId, true), "\"enabled\":true",
                "serve listener enable");
        String routeRemoved = serve.webRouteRemoveJson(listenerId, "docs");
        assertNotContains(routeRemoved, "\"route_id\":\"docs\"", "serve route remove");
        assertContains(serve.listenerRemoveJson(listenerId), listenerId, "serve listener remove");
    }

    private static void verifyOperationalPersistence(LoomSession session) {
        String workspaces = session.workspaces().listJson();
        assertContains(workspaces, "\"ops-studio\"", "persisted studio workspace");
        assertContains(workspaces, "\"ops-ai\"", "persisted inference workspace");
        assertContains(workspaces, "\"ops-site\"", "persisted site workspace");
        assertContains(workspaces, "\"bundle-src\"", "persisted bundle import workspace");
        assertNotContains(session.serveConfig().listenerListJson(), "\"127.0.0.1:19180\"",
                "removed listener persistence");
    }

    private static void verifyInterchange(LoomSession session) {
        session.workspaces().create("interop", "vcs");
        InterchangeProfilesOps interchange = session.interchangeProfiles();
        byte[] tablePayload = bytes("id,name,note\n1,alpha,\"contains, comma\"\n2,beta,\u0000nul\n");
        byte[] dryTable = interchange.importTableCsv("interop", "memory://table-dry.csv",
                tablePayload, "app", "dry_items", "id:int,name:text,note:text", "id",
                "snapshot", true, "jvm", "dry table", true);
        assertTrue(contains(dryTable, bytes("table-csv")), "dry table profile");
        assertTrue(contains(dryTable, bytes("memory://table-dry.csv")), "dry table source");
        assertThrows(() -> {
            try (Loom.LoomSql sql = session.sql("interop", "app")) {
                close(sql.exec("SELECT id FROM dry_items"));
            }
        }, "dry table did not persist");

        byte[] table = interchange.importTableCsv("interop", "memory://table.csv",
                tablePayload, "app", "items", "id:int,name:text,note:text", "id",
                "snapshot", true, "jvm", "table", false);
        assertTrue(contains(table, bytes("table-csv")), "table profile");
        assertTrue(contains(table, bytes("memory://table.csv")), "table source");

        byte[] ticketPayload = bytes("""
                {
                  "source_scope": "redmine://jvm",
                  "projects": [{"id": 1, "identifier": "core", "key_prefix": "CORE", "name": "Core"}],
                  "issues": [{
                    "id": 42,
                    "project_identifier": "core",
                    "tracker": "Bug",
                    "subject": "Login fails",
                    "description": "Fails on Safari",
                    "status": "New",
                    "priority": "High",
                    "custom_fields": {"severity": "critical"}
                  }]
                }""");
        assertThrows(() -> interchange.importRedmine("interop", "studio", "redmine://strict",
                ticketPayload, "strict", false), "redmine strict rejects undeclared fields");
        byte[] dryTicket = interchange.importRedmine("interop", "studio", "redmine://dry",
                ticketPayload, "infer", true);
        assertTrue(contains(dryTicket, bytes("redmine")), "dry redmine profile");
        assertTrue(contains(dryTicket, bytes("redmine://jvm")), "dry redmine payload scope");
        byte[] ticket = interchange.importRedmine("interop", "studio", "redmine://jvm",
                ticketPayload, "infer", false);
        assertTrue(contains(ticket, bytes("redmine")), "redmine profile");
        assertTrue(contains(ticket, bytes("redmine://jvm")), "redmine source");

        byte[] confluencePayload = bytes("""
                {
                  "source_scope": "confluence://jvm",
                  "spaces": [{"id": "wiki", "name": "Wiki"}],
                  "pages": [{"id": "home", "title": "Home", "space_id": "wiki", "text": "Hello docs"}]
                }""");
        byte[] dryContent = interchange.importConfluence("interop", "pages",
                "confluence://dry", confluencePayload, "wiki", true);
        assertTrue(contains(dryContent, bytes("confluence")), "dry confluence profile");
        assertNotContains(session.pages().pagesListJson("interop", "pages"),
                "\"home\"", "dry confluence did not persist");
        byte[] content = interchange.importConfluence("interop", "pages",
                "confluence://jvm", confluencePayload, "wiki", false);
        assertTrue(contains(content, bytes("confluence")), "confluence profile");
        assertTrue(contains(content, bytes("confluence://jvm")), "confluence source");
    }

    private static void verifyInterchangePersistence(LoomSession session) {
        assertContains(session.tickets().listJson("interop", "studio", "{\"kind\":\"flat\"}"),
                "\"external_source\":\"redmine\"", "persisted redmine ticket");
        assertContains(session.pages().pagesListJson("interop", "pages"),
                "\"home\"", "persisted confluence page");
    }

    private static void verifyDataExecution(LoomSession session) {
        session.workspaces().create("data-exec", "vcs");
        byte[] create = session.sqlExecResult("data-exec", "main",
                "CREATE TABLE items (id INTEGER, value TEXT)");
        assertTrue(create.length > 0, "sql exec result create bytes");
        byte[] insert = session.sqlExecResult("data-exec", "main",
                "INSERT INTO items VALUES (1, 'alpha')");
        assertTrue(insert.length > 0, "sql exec result insert bytes");
        byte[] select = session.sqlExecResult("data-exec", "main",
                "SELECT id, value FROM items ORDER BY id");
        assertTrue(contains(select, bytes("alpha")), "sql exec result selected value");

        ColumnarOps columnar = session.columnar();
        byte[] arbitraryPayload = new byte[] {0, 1, 2, 3, 0, (byte) 255};
        assertThrows(() -> columnar.importArrow("data-exec", "arrow", arbitraryPayload,
                7, false, true), "malformed arrow import rejected");
        assertThrows(() -> columnar.importParquet("data-exec", "parquet", arbitraryPayload,
                7, false, true), "malformed parquet import rejected");

        VectorOps vector = session.vector();
        assertThrows(() -> vector.textUpsert(new byte[] {1, 2, 0, 3}),
                "malformed vector text upsert rejected");
        assertThrows(() -> vector.workspaceConfigureJson("data-exec", "{}"),
                "malformed vector configure rejected");
    }

    private static void verifyDataExecutionPersistence(LoomSession session) {
        byte[] select = session.sqlExecResult("data-exec", "main",
                "SELECT id, value FROM items ORDER BY id");
        assertTrue(contains(select, bytes("alpha")), "sql exec result persisted selected value");
    }

    private static void verifyDriveReadHierarchy(LoomSession session) {
        session.workspaces().create("studio", "vcs");
        DriveOps drive = session.drive();
        String root = drive.listJson("studio", "drive-main", "root");
        assertContains(root, "\"folder_id\":\"root\"", "drive root folder");
        assertContains(root, "\"entries\":[]", "drive empty root");
        assertEquals("[]", drive.listSharesJson("studio", "drive-main"), "empty drive shares");
        assertEquals("[]", drive.listRetentionJson("studio", "drive-main"), "empty drive retention");

        String folderA = drive.createFolderJson("studio", "drive-main", "root", "folder-a",
                "A", jsonStringField(root, "profile_root"));
        assertContains(folderA, "\"target_entity_id\":\"folder-a\"", "drive create folder");
        assertThrows(() -> drive.createFolderJson("studio", "drive-main", "root", "stale",
                "Stale", jsonStringField(root, "profile_root")), "drive stale create folder");

        String renamed = drive.renameJson("studio", "drive-main", "root", "folder-a",
                "A2", jsonStringField(folderA, "profile_root"));
        assertContains(renamed, "\"target_entity_id\":\"folder-a\"", "drive rename folder");
        assertContains(drive.statJson("studio", "drive-main", "root", "A2"),
                "\"node_id\":\"folder-a\"", "drive stat renamed folder");

        String folderB = drive.createFolderJson("studio", "drive-main", "root", "folder-b",
                "B", jsonStringField(renamed, "profile_root"));
        String moved = drive.moveJson("studio", "drive-main", "root", "folder-b",
                "folder-a", jsonStringField(folderB, "profile_root"));
        assertContains(moved, "\"target_entity_id\":\"folder-a\"", "drive move folder");
        String heldDelete = drive.deleteJson("studio", "drive-main", "folder-b", "folder-a",
                jsonStringField(renamed, "profile_root"));
        assertContains(heldDelete, "\"operation_kind\":\"folder.delete_held\"", "drive held delete");
        assertContains(drive.listConflictsJson("studio", "drive-main"),
                "\"conflict_id\"", "drive conflict listing");
        String resolved = drive.resolveConflictJson("studio", "drive-main",
                jsonStringField(heldDelete, "conflict_id"), "keep_current");
        assertContains(resolved, "\"operation_kind\":\"conflict.resolved\"", "drive resolve conflict");
        String deleted = drive.deleteJson("studio", "drive-main", "folder-b", "folder-a",
                jsonStringField(resolved, "profile_root"));
        assertContains(deleted, "\"target_entity_id\":\"folder-a\"", "drive delete folder");

        String upload = drive.createUploadJson("studio", "drive-main", "upload-1", "root",
                "nul.bin", "file-1", jsonStringField(deleted, "profile_root"), 1000L, false);
        assertContains(upload, "\"upload_id\":\"upload-1\"", "drive create upload");
        byte[] payload = new byte[] {0x64, 0x72, 0x69, 0x76, 0x65, 0x00, 0x62, 0x79, 0x74, 0x65, 0x73};
        assertContains(drive.uploadChunkJson("studio", "drive-main", "upload-1", payload),
                "\"upload_id\":\"upload-1\"", "drive upload chunk");
        String committed = drive.commitUploadJson("studio", "drive-main", "upload-1");
        assertContains(committed, "\"target_entity_id\":\"file-1\"", "drive commit upload");
        assertBytes(payload, drive.readFile("studio", "drive-main", "file-1"), "drive read file bytes");
        assertContains(drive.listVersionsJson("studio", "drive-main", "file-1"),
                "\"version\":1", "drive versions list");

        assertThrows(() -> drive.grantShareJson("studio", "drive-main", "grant-zero", "file",
                "file-1", "05050505-0505-4505-8505-050505050505", "editor", 2000L, 0L),
                "drive zero share expiry remains present");
        String grant = drive.grantShareJson("studio", "drive-main", "grant-1", "file",
                "file-1", "05050505-0505-4505-8505-050505050505", "editor", 2000L, 2500L);
        assertContains(grant, "\"operation_kind\":\"share.granted\"", "drive grant share with zero expiry");
        assertContains(drive.listSharesJson("studio", "drive-main"),
                "\"grant_id\":\"grant-1\"", "drive share listing");
        assertContains(drive.applyShareExpiryJson("studio", "drive-main", 2100L),
                "\"remaining_grants\":1", "drive share no-op expiry");
        String revoked = drive.revokeShareJson("studio", "drive-main", "grant-1");
        assertContains(revoked, "\"operation_kind\":\"share.revoked\"", "drive revoke share");
        String expiringGrant = drive.grantShareJson("studio", "drive-main", "grant-expiring",
                "file", "file-1", "05050505-0505-4505-8505-050505050505", "viewer", 2200L, 2300L);
        assertContains(expiringGrant, "\"operation_kind\":\"share.granted\"", "drive grant expiring share");
        assertContains(drive.applyShareExpiryJson("studio", "drive-main", 2300L),
                "\"expired_grant_ids\":[\"grant-expiring\"]", "drive apply share expiry");

        String pinned = drive.pinRetentionJson("studio", "drive-main", "pin-1", "legal_hold",
                jsonStringField(committed, "profile_root"), "file:file-1", 3000L, null);
        assertContains(pinned, "\"operation_kind\":\"retention.pinned\"", "drive pin retention");
        assertContains(drive.applyRetentionJson("studio", "drive-main", 3100L),
                "\"remaining_pins\":1", "drive retention no expiry");
        String unpinned = drive.unpinRetentionJson("studio", "drive-main", "pin-1");
        assertContains(unpinned, "\"operation_kind\":\"retention.unpinned\"", "drive unpin retention");
        String expiringPin = drive.pinRetentionJson("studio", "drive-main", "pin-expiring",
                "trash_subtree", jsonStringField(committed, "profile_root"), "file:file-1", 3200L, 3300L);
        assertContains(expiringPin, "\"operation_kind\":\"retention.pinned\"", "drive pin expiring retention");
        assertContains(drive.applyRetentionJson("studio", "drive-main", 3300L),
                "\"expired_pin_ids\":[\"pin-expiring\"]", "drive apply retention expiry");
    }

    private static void verifyDriveReadHierarchyPersistence(LoomSession session) {
        DriveOps drive = session.drive();
        assertContains(drive.listJson("studio", "drive-main", "root"),
                "\"node_id\":\"file-1\"", "drive persisted file entry");
        assertBytes(new byte[] {0x64, 0x72, 0x69, 0x76, 0x65, 0x00, 0x62, 0x79, 0x74, 0x65, 0x73},
                drive.readFile("studio", "drive-main", "file-1"), "drive persisted file bytes");
    }

    private static byte[] minimalBundle() {
        return new byte[] {
                (byte) 0x89, 0x66, 0x4c, 0x4d, 0x42, 0x4e, 0x44, 0x4c, 0x04, 0x18, 0x1e,
                0x50, 0x3d, 0x3d, 0x3d, 0x3d, 0x3d, 0x3d, 0x3d, 0x3d,
                0x3d, 0x3d, 0x3d, 0x3d, 0x3d, 0x3d, 0x3d, 0x3d,
                (byte) 0x81, 0x63, 0x76, 0x63, 0x73,
                0x6a, 0x62, 0x75, 0x6e, 0x64, 0x6c, 0x65, 0x2d, 0x73, 0x72, 0x63,
                (byte) 0x80, (byte) 0x80, (byte) 0x80
        };
    }

    private static String jsonStringField(String json, String field) {
        String marker = "\"" + field + "\":\"";
        int start = json.indexOf(marker);
        assertTrue(start >= 0, "json field " + field);
        start += marker.length();
        int end = json.indexOf('"', start);
        assertTrue(end > start, "json field value " + field);
        return json.substring(start, end);
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
