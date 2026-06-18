import CUldrenLoom
import Foundation

extension Loom {
    /// Create vector set `name` of width `dim` and `metric` (1 cosine, 2 L2, 3 dot) in `workspace` (created
    /// with the `vector` facet if absent). Conflicts if a set of that name already exists.
    public func vectorCreate(workspace: String, name: String, dim: Int, metric: Int32) throws {
        let status = loom_vector_create(session, workspace, name, UInt(dim), metric)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Insert or replace the vector at `id` in set `name`. `vector` is little-endian f32 bytes (4 per
    /// component); `metadata` is a Loom Canonical CBOR map of `text -> cell`, an empty `Data` meaning none.
    public func vectorUpsert(workspace: String, name: String, id: String, vector: Data,
                             metadata: Data = Data()) throws {
        let status = vector.withUnsafeBytes { vraw -> Int32 in
            let vbase = vraw.bindMemory(to: UInt8.self).baseAddress
            return metadata.withUnsafeBytes { mraw -> Int32 in
                let mbase = mraw.bindMemory(to: UInt8.self).baseAddress
                return loom_vector_upsert(session, workspace, name, id, vbase, UInt(vraw.count), mbase,
                                          UInt(mraw.count))
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Insert or replace a vector with UTF-8 source text and optional embedding model profile.
    public func vectorUpsertSource(workspace: String, name: String, id: String, vector: Data,
                                   metadata: Data = Data(), sourceText: Data,
                                   modelId: String? = nil, weightsDigest: String? = nil) throws {
        let status = vector.withUnsafeBytes { vraw -> Int32 in
            let vbase = vraw.bindMemory(to: UInt8.self).baseAddress
            return metadata.withUnsafeBytes { mraw -> Int32 in
                let mbase = mraw.bindMemory(to: UInt8.self).baseAddress
                return sourceText.withUnsafeBytes { sraw -> Int32 in
                    let sbase = sraw.bindMemory(to: UInt8.self).baseAddress
                    return loom_vector_upsert_source(
                        session, workspace, name, id, vbase, UInt(vraw.count), mbase,
                        UInt(mraw.count), sbase, UInt(sraw.count), modelId, modelId == nil ? 0 : 1,
                        weightsDigest, weightsDigest == nil ? 0 : 1)
                }
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch the vector + metadata at `id` in set `name` as the Loom Canonical CBOR array
    /// `[vector_bytes, metadata]`, or nil if the id is absent.
    public func vectorGet(workspace: String, name: String, id: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_vector_get(session, workspace, name, id, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Fetch UTF-8 source text for vector `id`, or nil if absent.
    public func vectorSourceText(workspace: String, name: String, id: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_vector_source_text(session, workspace, name, id, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Fetch the embedding model profile as CBOR `[1, model_id, dimension, weights_digest]`, or nil.
    public func vectorEmbeddingModel(workspace: String, name: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_vector_embedding_model_cbor(session, workspace, name, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Vector ids in set `name`, sorted ascending, as the Loom Canonical CBOR array of text.
    public func vectorIds(workspace: String, name: String, prefix: String? = nil) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_vector_ids_cbor(session, workspace, name, prefix, prefix == nil ? 0 : 1, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Declared metadata equality index keys for set `name`, sorted ascending, as a Loom Canonical CBOR
    /// array of text.
    public func vectorMetadataIndexKeys(workspace: String, name: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_vector_metadata_index_keys_cbor(session, workspace, name, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Declare and build a metadata equality index for `key`; returns whether a new index was declared.
    public func vectorCreateMetadataIndex(workspace: String, name: String, key: String) throws -> Bool {
        var changed: Int32 = 0
        let status = loom_vector_create_metadata_index(session, workspace, name, key, &changed)
        guard status == 0 else { throw LoomSql.lastError() }
        return changed != 0
    }

    /// Drop the metadata equality index for `key`; returns whether an index was present.
    public func vectorDropMetadataIndex(workspace: String, name: String, key: String) throws -> Bool {
        var changed: Int32 = 0
        let status = loom_vector_drop_metadata_index(session, workspace, name, key, &changed)
        guard status == 0 else { throw LoomSql.lastError() }
        return changed != 0
    }

    /// Remove `id` from set `name`; returns whether it was present.
    public func vectorDelete(workspace: String, name: String, id: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_vector_delete(session, workspace, name, id, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// The exact top-`k` nearest neighbours of `query` (little-endian f32 bytes) among vectors of set `name`
    /// passing `filter`, as the Loom Canonical CBOR array of `[id, score_cell]`, highest score first. An empty
    /// `filter` matches all.
    public func vectorSearch(workspace: String, name: String, query: Data, k: Int,
                             filter: Data = Data()) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = query.withUnsafeBytes { qraw -> Int32 in
            let qbase = qraw.bindMemory(to: UInt8.self).baseAddress
            return filter.withUnsafeBytes { fraw -> Int32 in
                let fbase = fraw.bindMemory(to: UInt8.self).baseAddress
                return loom_vector_search_cbor(session, workspace, name, qbase, UInt(qraw.count), UInt(k),
                                               fbase, UInt(fraw.count), &ptr, &len)
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Top-k vector search with explicit accelerator policy over built-in PQ. `policy` is 0 exact
    /// and 1 approximate-above-threshold. Result CBOR matches `vectorSearch`.
    public func vectorSearchPolicy(workspace: String, name: String, query: Data, k: Int,
                                   filter: Data = Data(), policy: Int32, threshold: Int, ef: Int,
                                   pqM: Int, pqK: Int, pqIters: Int) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = query.withUnsafeBytes { qraw -> Int32 in
            let qbase = qraw.bindMemory(to: UInt8.self).baseAddress
            return filter.withUnsafeBytes { fraw -> Int32 in
                let fbase = fraw.bindMemory(to: UInt8.self).baseAddress
                return loom_vector_search_policy_cbor(
                    session, workspace, name, qbase, UInt(qraw.count), UInt(k), fbase,
                    UInt(fraw.count), policy, UInt(threshold), UInt(ef), UInt(pqM), UInt(pqK),
                    UInt(pqIters), &ptr, &len)
            }
        }
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }
}
