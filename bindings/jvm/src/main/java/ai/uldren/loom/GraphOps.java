package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

/**
 * Graph facet operations for a {@link LoomSession}: nodes and labelled edges, one set per named graph,
 * in a workspace. Reached via {@link LoomSession#graph()}. Props cross as Loom Canonical CBOR maps of
 * {@code text -> bytes}; a node get returns the props CBOR; an edge get returns the CBOR array
 * {@code [src, dst, label, props]}; traversal calls return CBOR arrays. An absent node, edge, or graph
 * reads as absent. Owns the FFM downcalls directly via {@link Loom#onHandle}.
 *
 * <p>Licensed under BUSL-1.1 (see the workspace {@code LICENSE}). (c) Uldren Technologies LLC.
 */
public final class GraphOps {
    private final LoomSession session;

    GraphOps(LoomSession session) {
        this.session = session;
    }

    /** Insert or replace node {@code id} in graph {@code name} (created if absent) with CBOR {@code props}. */
    public void upsertNode(String workspace, String name, String id, byte[] props) {
        session.onHandle("loom_graph_upsert_node",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_GRAPH_UPSERT_NODE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), Loom.bytesOrNull(arena, props),
                            (long) (props != null ? props.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_upsert_node");
                    }
                    return null;
                });
    }

    /** Fetch node {@code id}'s props as a CBOR map in graph {@code name}, or {@code null} if absent. */
    public byte[] getNode(String workspace, String name, String id) {
        return session.onHandle("loom_graph_get_node",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_GRAPH_GET_NODE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_get_node");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Remove node {@code id} from graph {@code name}; {@code cascade} also removes its incident edges. */
    public void removeNode(String workspace, String name, String id, boolean cascade) {
        session.onHandle("loom_graph_remove_node",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_GRAPH_REMOVE_NODE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), cascade ? 1 : 0);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_remove_node");
                    }
                    return null;
                });
    }

    /** Insert or replace edge {@code id} from {@code src} to {@code dst} with {@code label} and CBOR {@code props}. */
    public void upsertEdge(String workspace, String name, String id, String src, String dst,
            String label, byte[] props) {
        session.onHandle("loom_graph_upsert_edge",
                (arena, handle) -> {
                    int status = (int) Loom.LOOM_GRAPH_UPSERT_EDGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), arena.allocateFrom(src), arena.allocateFrom(dst),
                            arena.allocateFrom(label), Loom.bytesOrNull(arena, props),
                            (long) (props != null ? props.length : 0));
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_upsert_edge");
                    }
                    return null;
                });
    }

    /** Fetch edge {@code id} as CBOR {@code [src, dst, label, props]} in graph {@code name}, or {@code null}. */
    public byte[] getEdge(String workspace, String name, String id) {
        return session.onHandle("loom_graph_get_edge",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_GRAPH_GET_EDGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outPtr, outLen, outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_get_edge");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Remove edge {@code id} from graph {@code name}; returns whether it was present. */
    public boolean removeEdge(String workspace, String name, String id) {
        return session.onHandle("loom_graph_remove_edge",
                (arena, handle) -> {
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_GRAPH_REMOVE_EDGE.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_remove_edge");
                    }
                    return outFound.get(ValueLayout.JAVA_INT, 0) != 0;
                });
    }

    /** Distinct adjacent node ids of {@code id}, sorted, as a CBOR array of text. */
    public byte[] neighbors(String workspace, String name, String id) {
        return session.onHandle("loom_graph_neighbors_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_GRAPH_NEIGHBORS_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_neighbors_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** Out-edges of {@code id} as a CBOR array of {@code [edge_id, edge]} in edge-id order. */
    public byte[] outEdges(String workspace, String name, String id) {
        return session.onHandle("loom_graph_out_edges_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_GRAPH_OUT_EDGES_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_out_edges_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /** In-edges of {@code id} as a CBOR array of {@code [edge_id, edge]} in edge-id order. */
    public byte[] inEdges(String workspace, String name, String id) {
        return session.onHandle("loom_graph_in_edges_cbor",
                (arena, handle) -> {
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_GRAPH_IN_EDGES_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(id), outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_in_edges_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /**
     * Node ids reachable from {@code start} as a CBOR array of text. {@code maxDepth < 0} means no
     * limit; {@code viaLabel} null follows every edge, else only edges with that label.
     */
    public byte[] reachable(String workspace, String name, String start, long maxDepth,
            String viaLabel) {
        return session.onHandle("loom_graph_reachable_cbor",
                (arena, handle) -> {
                    MemorySegment viaSeg =
                            viaLabel != null ? arena.allocateFrom(viaLabel) : MemorySegment.NULL;
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    int status = (int) Loom.LOOM_GRAPH_REACHABLE_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(start), maxDepth, viaSeg, outPtr, outLen);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_reachable_cbor");
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }

    /**
     * A shortest path from {@code from} to {@code to} as a CBOR array of node-id text, or {@code null}
     * if none exists. {@code viaLabel} null follows every edge, else only edges with that label.
     */
    public byte[] shortestPath(String workspace, String name, String from, String to,
            String viaLabel) {
        return session.onHandle("loom_graph_shortest_path_cbor", (arena, handle) -> {
                    MemorySegment viaSeg =
                            viaLabel != null ? arena.allocateFrom(viaLabel) : MemorySegment.NULL;
                    MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
                    MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
                    MemorySegment outFound = arena.allocate(ValueLayout.JAVA_INT);
                    int status = (int) Loom.LOOM_GRAPH_SHORTEST_PATH_CBOR.invokeExact(handle,
                            arena.allocateFrom(workspace), arena.allocateFrom(name),
                            arena.allocateFrom(from), arena.allocateFrom(to), viaSeg, outPtr, outLen,
                            outFound);
                    if (status != 0) {
                        throw Loom.lastError("loom_graph_shortest_path_cbor");
                    }
                    if (outFound.get(ValueLayout.JAVA_INT, 0) == 0) {
                        return null;
                    }
                    return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                            outLen.get(ValueLayout.JAVA_LONG, 0));
                });
    }
}
