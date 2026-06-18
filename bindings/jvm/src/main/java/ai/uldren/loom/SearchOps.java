package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Search facet operations for a {@link LoomSession}: full-text/structured documents, one collection per
 * named index, in a workspace. Reached via {@link LoomSession#search()}. The field {@code mapping}
 * crosses as a Loom Canonical CBOR map of {@code field -> [type_tag, stored, faceted]}; an id is opaque
 * bytes; a document crosses as a CBOR map of {@code field -> value}; a get returns the document CBOR;
 * {@code ids} returns a CBOR array of byte strings; a query takes the CBOR {@code [query, limit, offset]}
 * and returns the response CBOR. An absent id reads as absent. Owns the FFM downcalls directly via
 * {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class SearchOps {
    private final LoomSession session;

    SearchOps(LoomSession session) {
        this.session = session;
    }

    /** Create search collection {@code name} with the field {@code mapping} (CBOR {@code field -> [type_tag, stored, faceted]}). */
    public void create(String workspace, String name, byte[] mapping) {
        session.onHandle("loom_search_create",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_SEARCH_CREATE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, mapping),
                            (long) (mapping != null ? mapping.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_search_create");
                    }
                    return null;
                });
    }

    /** Insert or replace the document at {@code id} (opaque bytes); {@code doc} is a CBOR {@code field -> value} map. */
    public void index(String workspace, String name, byte[] id, byte[] doc) {
        session.onHandle("loom_search_index",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_SEARCH_INDEX.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, id), (long) (id != null ? id.length : 0),
                            Loom.bytesOrNull(arena, doc), (long) (doc != null ? doc.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_search_index");
                    }
                    return null;
                });
    }

    /** Fetch the document at {@code id} as a CBOR {@code field -> value} map, or {@code null} if absent. */
    public byte[] get(String workspace, String name, byte[] id) {
        return session.onHandle("loom_search_get",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_SEARCH_GET.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, id), (long) (id != null ? id.length : 0),
                            outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_search_get");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Remove the document at {@code id}; returns whether it was present. */
    public boolean delete(String workspace, String name, byte[] id) {
        return session.onHandle("loom_search_delete",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_SEARCH_DELETE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, id), (long) (id != null ? id.length : 0),
                            outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_search_delete");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /**
     * Document ids as a CBOR array of byte strings; when {@code hasPrefix} is true the listing is
     * restricted to ids under {@code prefix}.
     */
    public byte[] ids(String workspace, String name, byte[] prefix, boolean hasPrefix) {
        return session.onHandle("loom_search_ids_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_SEARCH_IDS_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, prefix),
                            (long) (prefix != null ? prefix.length : 0), hasPrefix ? 1 : 0,
                            outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_search_ids_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Replace the field {@code mapping} of collection {@code name} (CBOR {@code field -> [type_tag, stored, faceted]}). */
    public void remap(String workspace, String name, byte[] mapping) {
        session.onHandle("loom_search_remap",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_SEARCH_REMAP.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, mapping),
                            (long) (mapping != null ? mapping.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_search_remap");
                    }
                    return null;
                });
    }

    /** Run the portable linear-scan query ({@code request} CBOR {@code [query, limit, offset]}) and return the response CBOR. */
    public byte[] query(String workspace, String name, byte[] request) {
        return session.onHandle("loom_search_query_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_SEARCH_QUERY_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, request),
                            (long) (request != null ? request.length : 0), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_search_query_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }
}
