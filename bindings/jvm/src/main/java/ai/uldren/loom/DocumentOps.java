package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.nio.charset.StandardCharsets;

/**
 * Document facet operations for a {@link LoomSession}. Text operations use UTF-8 strings and binary
 * operations use opaque bytes.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class DocumentOps {
    private final LoomSession session;

    public record Text(String text, String digest, String entityTag) {
    }

    public record Binary(byte[] bytes, String digest, String entityTag) {
    }

    public record PutResult(String digest, String entityTag) {
    }

    DocumentOps(LoomSession session) {
        this.session = session;
    }

    public PutResult putText(String workspace, String collection, String id, String text) {
        return putText(workspace, collection, id, text, null);
    }

    public PutResult putText(String workspace, String collection, String id, String text, String expectedEntityTag) {
        return session.onHandle("loom_doc_put_text", (arena, handle) -> {
            MemorySegment outDigest = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outEntityTag = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_DOC_PUT_TEXT.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(collection), arena.allocateFrom(id), arena.allocateFrom(text),
                    cStringOrNull(arena, expectedEntityTag), outDigest, outEntityTag);
            if (status != 0) {
                throw Loom.lastError("loom_doc_put_text");
            }
            return new PutResult(Loom.takeOwnedString(outDigest.get(ValueLayout.ADDRESS, 0)),
                    Loom.takeOwnedString(outEntityTag.get(ValueLayout.ADDRESS, 0)));
        });
    }

    public Text getText(String workspace, String collection, String id) {
        return session.onHandle("loom_doc_get_text",
                (arena, handle) -> {
                    MemorySegment outText = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outDigest = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outEntityTag = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_DOC_GET_TEXT.invokeExact(handle, arena.allocateFrom(workspace),
                            arena.allocateFrom(collection), arena.allocateFrom(id), outText, outDigest, outEntityTag,
                            outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_doc_get_text");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return new Text(Loom.takeOwnedString(outText.get(ValueLayout.ADDRESS, 0)),
                            Loom.takeOwnedString(outDigest.get(ValueLayout.ADDRESS, 0)),
                            Loom.takeOwnedString(outEntityTag.get(ValueLayout.ADDRESS, 0)));
                });
    }

    public PutResult putBinary(String workspace, String collection, String id, byte[] bytes) {
        return putBinary(workspace, collection, id, bytes, null);
    }

    public PutResult putBinary(String workspace, String collection, String id, byte[] bytes, String expectedEntityTag) {
        return session.onHandle("loom_doc_put_binary", (arena, handle) -> {
            MemorySegment outDigest = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outEntityTag = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_DOC_PUT_BINARY.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(collection), arena.allocateFrom(id), Loom.bytesOrNull(arena, bytes),
                    (long) (bytes != null ? bytes.length : 0), cStringOrNull(arena, expectedEntityTag),
                    outDigest, outEntityTag);
            if (status != 0) {
                throw Loom.lastError("loom_doc_put_binary");
            }
            return new PutResult(Loom.takeOwnedString(outDigest.get(ValueLayout.ADDRESS, 0)),
                    Loom.takeOwnedString(outEntityTag.get(ValueLayout.ADDRESS, 0)));
        });
    }

    public Binary getBinary(String workspace, String collection, String id) {
        return session.onHandle("loom_doc_get_binary", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment outDigest = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outEntityTag = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
            int status = (int) Loom.LOOM_DOC_GET_BINARY.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(collection), arena.allocateFrom(id), outPtr, outLen, outDigest, outEntityTag,
                    outFound);
            if (status != 0) {
                throw Loom.lastError("loom_doc_get_binary");
            }
            if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                return null;
            }
            byte[] bytes = Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
            return new Binary(bytes, Loom.takeOwnedString(outDigest.get(ValueLayout.ADDRESS, 0)),
                    Loom.takeOwnedString(outEntityTag.get(ValueLayout.ADDRESS, 0)));
        });
    }

    /** Remove {@code id} from collection {@code collection}; returns whether it was present. */
    public boolean delete(String workspace, String collection, String id) {
        return session.onHandle("loom_doc_delete",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_DOC_DELETE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection),
                            arena.allocateFrom(id), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_doc_delete");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    public byte[] listBinary(String workspace, String collection) {
        return session.onHandle("loom_doc_list_binary_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_DOC_LIST_BINARY_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(collection), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_doc_list_binary_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    public void indexCreate(String workspace, String collection, String name, String path, boolean unique) {
        session.onHandle("loom_doc_index_create", (arena, handle) -> {
            int status = (int) Loom.LOOM_DOC_INDEX_CREATE.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(collection), arena.allocateFrom(name),
                    arena.allocateFrom(path), unique ? 1 : 0);
            if (status != 0) {
                throw Loom.lastError("loom_doc_index_create");
            }
            return null;
        });
    }

    public void indexCreateJson(String workspace, String collection, byte[] declarationJson) {
        session.onHandle("loom_doc_index_create_json", (arena, handle) -> {
            MemorySegment declaration = arena.allocate(Math.max(declarationJson.length, 1));
            declaration.copyFrom(MemorySegment.ofArray(declarationJson));
            int status = (int) Loom.LOOM_DOC_INDEX_CREATE_JSON.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(collection), declaration,
                    (long) declarationJson.length);
            if (status != 0) {
                throw Loom.lastError("loom_doc_index_create_json");
            }
            return null;
        });
    }

    public boolean indexDrop(String workspace, String collection, String name) {
        return session.onHandle("loom_doc_index_drop", (arena, handle) -> {
            MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
            int status = (int) Loom.LOOM_DOC_INDEX_DROP.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(collection), arena.allocateFrom(name),
                    outFound);
            if (status != 0) {
                throw Loom.lastError("loom_doc_index_drop");
            }
            return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
        });
    }

    public void indexRebuild(String workspace, String collection, String name) {
        session.onHandle("loom_doc_index_rebuild", (arena, handle) -> {
            int status = (int) Loom.LOOM_DOC_INDEX_REBUILD.invokeExact(handle,
                    arena.allocateFrom(workspace), arena.allocateFrom(collection), arena.allocateFrom(name));
            if (status != 0) {
                throw Loom.lastError("loom_doc_index_rebuild");
            }
            return null;
        });
    }

    public String indexListJson(String workspace, String collection) {
        return documentJson("loom_doc_index_list_json", (arena, handle, outPtr, outLen) ->
                (int) Loom.LOOM_DOC_INDEX_LIST_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                        arena.allocateFrom(collection), outPtr, outLen));
    }

    public String indexStatusJson(String workspace, String collection) {
        return documentJson("loom_doc_index_status_json", (arena, handle, outPtr, outLen) ->
                (int) Loom.LOOM_DOC_INDEX_STATUS_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                        arena.allocateFrom(collection), outPtr, outLen));
    }

    public String findJson(String workspace, String collection, String index, String valueJson) {
        byte[] raw = valueJson.getBytes(StandardCharsets.UTF_8);
        return documentJson("loom_doc_find_json", (arena, handle, outPtr, outLen) ->
                (int) Loom.LOOM_DOC_FIND_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                        arena.allocateFrom(collection), arena.allocateFrom(index), Loom.bytesOrNull(arena, raw),
                        (long) raw.length, outPtr, outLen));
    }

    public String queryJson(String workspace, String collection, String queryJson) {
        byte[] raw = queryJson.getBytes(StandardCharsets.UTF_8);
        return documentJson("loom_doc_query_json", (arena, handle, outPtr, outLen) ->
                (int) Loom.LOOM_DOC_QUERY_JSON.invokeExact(handle, arena.allocateFrom(workspace),
                        arena.allocateFrom(collection), Loom.bytesOrNull(arena, raw), (long) raw.length,
                        outPtr, outLen));
    }

    private String documentJson(String label, DocumentJsonCall call) {
        return session.onHandle(label, (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = call.invoke(arena, handle, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError(label);
            }
            byte[] bytes = Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
            return new String(bytes, StandardCharsets.UTF_8);
        });
    }

    private static MemorySegment cStringOrNull(Arena arena, String value) {
        return value == null ? MemorySegment.NULL : arena.allocateFrom(value);
    }

    @FunctionalInterface
    private interface DocumentJsonCall {
        int invoke(Arena arena, MemorySegment handle, MemorySegment outPtr, MemorySegment outLen) throws Throwable;
    }
}
