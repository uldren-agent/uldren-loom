import CUldrenLoom
import Foundation

extension Loom {
    /// Insert or replace node `id` in graph `name` of `workspace` (created with the `graph` facet if absent).
    /// `props` is a Loom Canonical CBOR map of `text -> bytes`; an empty `Data` means no properties.
    public func graphUpsertNode(workspace: String, name: String, id: String, props: Data = Data()) throws {
        let status = props.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_graph_upsert_node(session, workspace, name, id, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch node `id`'s properties in graph `name` as a CBOR map of `text -> bytes`, or nil if the node is absent.
    public func graphGetNode(workspace: String, name: String, id: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_graph_get_node(session, workspace, name, id, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Remove node `id` from graph `name`. When `cascade` is true, incident edges are also removed; otherwise
    /// removing a node while any edge still touches it is a conflict.
    public func graphRemoveNode(workspace: String, name: String, id: String, cascade: Bool = false) throws {
        let status = loom_graph_remove_node(session, workspace, name, id, cascade ? 1 : 0)
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Insert or replace edge `id` from node `src` to node `dst` (both must exist) in graph `name` with `label`.
    /// `props` is a CBOR map of `text -> bytes`; an empty `Data` means no properties.
    public func graphUpsertEdge(workspace: String, name: String, id: String, src: String, dst: String,
                                label: String, props: Data = Data()) throws {
        let status = props.withUnsafeBytes { raw -> Int32 in
            let base = raw.bindMemory(to: UInt8.self).baseAddress
            return loom_graph_upsert_edge(session, workspace, name, id, src, dst, label, base, UInt(raw.count))
        }
        guard status == 0 else { throw LoomSql.lastError() }
    }

    /// Fetch edge `id` in graph `name` as the Loom Canonical CBOR array `[src, dst, label, props]`, or nil if absent.
    public func graphGetEdge(workspace: String, name: String, id: String) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_graph_get_edge(session, workspace, name, id, &ptr, &len, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// Remove edge `id` from graph `name`; returns whether it was present.
    public func graphRemoveEdge(workspace: String, name: String, id: String) throws -> Bool {
        var found: Int32 = 0
        let status = loom_graph_remove_edge(session, workspace, name, id, &found)
        guard status == 0 else { throw LoomSql.lastError() }
        return found != 0
    }

    /// The distinct adjacent node ids of `id` in graph `name`, sorted, as the Loom Canonical CBOR array of text.
    public func graphNeighbors(workspace: String, name: String, id: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_graph_neighbors_cbor(session, workspace, name, id, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The out-edges of `id` in graph `name` as the Loom Canonical CBOR array of `[edge_id, edge]` in edge-id order.
    public func graphOutEdges(workspace: String, name: String, id: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_graph_out_edges_cbor(session, workspace, name, id, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The in-edges of `id` in graph `name` as the Loom Canonical CBOR array of `[edge_id, edge]` in edge-id order.
    public func graphInEdges(workspace: String, name: String, id: String) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_graph_in_edges_cbor(session, workspace, name, id, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// The node ids reachable from `start` in graph `name` as the Loom Canonical CBOR array of text.
    /// A negative `maxDepth` means no depth limit; a nil `viaLabel` follows every edge, else only edges with
    /// that label.
    public func graphReachable(workspace: String, name: String, start: String, maxDepth: Int64 = -1,
                               viaLabel: String? = nil) throws -> Data {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        let status = loom_graph_reachable_cbor(session, workspace, name, start, maxDepth, viaLabel, &ptr, &len)
        guard status == 0 else { throw LoomSql.lastError() }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }

    /// A shortest path from `from` to `to` in graph `name` as the Loom Canonical CBOR array of node-id text, or
    /// nil if no path exists. A nil `viaLabel` follows every edge, else only edges with that label.
    public func graphShortestPath(workspace: String, name: String, from: String, to: String,
                                  viaLabel: String? = nil) throws -> Data? {
        var ptr: UnsafeMutablePointer<UInt8>?
        var len: UInt = 0
        var found: Int32 = 0
        let status = loom_graph_shortest_path_cbor(session, workspace, name, from, to, viaLabel, &ptr, &len,
                                                   &found)
        guard status == 0 else { throw LoomSql.lastError() }
        guard found != 0 else { return nil }
        defer { loom_bytes_free(ptr, len) }
        guard let ptr, len > 0 else { return Data() }
        return Data(UnsafeBufferPointer(start: ptr, count: Int(len)))
    }
}
