package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Vector facet operations for a {@link LoomSession}: embeddings with metadata, one set per named
 * collection, in a workspace. Reached via {@link LoomSession#vector()}. An embedding crosses as raw
 * little-endian {@code f32} bytes (4 per component); metadata crosses as a Loom Canonical CBOR map of
 * {@code text -> cell}; a get returns the CBOR array {@code [vector_bytes, metadata]}; a search returns
 * a CBOR array of {@code [id, score_cell]}, highest score first. The {@code metric}: 1 cosine, 2 L2,
 * 3 dot. An absent id reads as absent. Owns the FFM downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class VectorOps {
    private final LoomSession session;

    VectorOps(LoomSession session) {
        this.session = session;
    }

    /** Create vector set {@code name} with embedding dimension {@code dim} and distance {@code metric}. */
    public void create(String workspace, String name, long dim, int metric) {
        session.onHandle("loom_vector_create",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_VECTOR_CREATE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), dim, metric);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_create");
                    }
                    return null;
                });
    }

    /** Insert or replace the vector at {@code id}: {@code vector} is LE f32 bytes; {@code metadata} a CBOR map. */
    public void upsert(String workspace, String name, String id, byte[] vector, byte[] metadata) {
        session.onHandle("loom_vector_upsert",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_VECTOR_UPSERT.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), Loom.bytesOrNull(arena, vector),
                            (long) (vector != null ? vector.length : 0),
                            Loom.bytesOrNull(arena, metadata),
                            (long) (metadata != null ? metadata.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_upsert");
                    }
                    return null;
                });
    }

    /** Insert or replace a vector with UTF-8 source text and optional embedding model profile. */
    public void upsertSource(String workspace, String name, String id, byte[] vector, byte[] metadata,
            byte[] sourceText, String modelId, String weightsDigest) {
        session.onHandle("loom_vector_upsert_source",
                (arena, handle) -> {
                    MemorySegment model = modelId != null ? arena.allocateFrom(modelId) : MemorySegment.NULL;
                    MemorySegment weights = weightsDigest != null ? arena.allocateFrom(weightsDigest)
                            : MemorySegment.NULL;
                    int status = (int) Loom.LOOM_VECTOR_UPSERT_SOURCE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), Loom.bytesOrNull(arena, vector),
                            (long) (vector != null ? vector.length : 0),
                            Loom.bytesOrNull(arena, metadata),
                            (long) (metadata != null ? metadata.length : 0),
                            Loom.bytesOrNull(arena, sourceText),
                            (long) (sourceText != null ? sourceText.length : 0),
                            model, modelId != null ? 1 : 0, weights, weightsDigest != null ? 1 : 0);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_upsert_source");
                    }
                    return null;
                });
    }

    /** Fetch the vector + metadata at {@code id} as CBOR {@code [vector_bytes, metadata]}, or {@code null}. */
    public byte[] get(String workspace, String name, String id) {
        return session.onHandle("loom_vector_get",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_VECTOR_GET.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_get");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Fetch UTF-8 source text bytes for {@code id}, or {@code null}. */
    public byte[] sourceText(String workspace, String name, String id) {
        return session.onHandle("loom_vector_source_text",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_VECTOR_SOURCE_TEXT.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_source_text");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Fetch the embedding model profile as CBOR {@code [1, model_id, dimension, weights_digest]}. */
    public byte[] embeddingModel(String workspace, String name) {
        return session.onHandle("loom_vector_embedding_model_cbor", (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_VECTOR_EMBEDDING_MODEL_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen,
                            outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_embedding_model_cbor");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Vector ids in ascending order as a CBOR array of text; {@code prefix} restricts by string prefix. */
    public byte[] ids(String workspace, String name, String prefix) {
        return session.onHandle("loom_vector_ids_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment prefixSeg = prefix != null ? arena.allocateFrom(prefix) : MemorySegment.NULL;
                    int status = (int) Loom.LOOM_VECTOR_IDS_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            prefixSeg, prefix != null ? 1 : 0, outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_ids_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Declared metadata equality index keys for {@code name}, sorted ascending as a CBOR array of text. */
    public byte[] metadataIndexKeys(String workspace, String name) {
        return session.onHandle("loom_vector_metadata_index_keys_cbor", (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_VECTOR_METADATA_INDEX_KEYS_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_metadata_index_keys_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Declare and build a metadata equality index for {@code key}; returns whether it was new. */
    public boolean createMetadataIndex(String workspace, String name, String key) {
        return session.onHandle("loom_vector_create_metadata_index", (arena, handle) -> {
                    MemorySegment outChanged = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_VECTOR_CREATE_METADATA_INDEX.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(key), outChanged);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_create_metadata_index");
                    }
                    return outChanged.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** Drop the metadata equality index for {@code key}; returns whether an index was present. */
    public boolean dropMetadataIndex(String workspace, String name, String key) {
        return session.onHandle("loom_vector_drop_metadata_index", (arena, handle) -> {
                    MemorySegment outChanged = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_VECTOR_DROP_METADATA_INDEX.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(key), outChanged);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_drop_metadata_index");
                    }
                    return outChanged.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** Remove the vector at {@code id}; returns whether it was present. */
    public boolean delete(String workspace, String name, String id) {
        return session.onHandle("loom_vector_delete",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_VECTOR_DELETE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_delete");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /**
     * Exact top-{@code k} nearest neighbours of {@code query} (LE f32 bytes) among vectors passing
     * {@code filter} (CBOR; empty or {@code null} = all) as a CBOR array of {@code [id, score_cell]}.
     */
    public byte[] search(String workspace, String name, byte[] query, long k, byte[] filter) {
        return session.onHandle("loom_vector_search_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_VECTOR_SEARCH_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, query), (long) (query != null ? query.length : 0),
                            k, Loom.bytesOrNull(arena, filter),
                            (long) (filter != null ? filter.length : 0), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_search_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /**
     * Top-k nearest neighbours with explicit accelerator policy over built-in PQ. {@code policy} is 0
     * exact and 1 approximate-above-threshold. Result CBOR matches {@link #search}.
     */
    public byte[] searchPolicy(String workspace, String name, byte[] query, long k, byte[] filter,
            int policy, long threshold, long ef, long pqM, long pqK, long pqIters) {
        return session.onHandle("loom_vector_search_policy_cbor", (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_VECTOR_SEARCH_POLICY_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            Loom.bytesOrNull(arena, query), (long) (query != null ? query.length : 0),
                            k, Loom.bytesOrNull(arena, filter),
                            (long) (filter != null ? filter.length : 0), policy, threshold, ef, pqM,
                            pqK, pqIters, outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_vector_search_policy_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }
}
