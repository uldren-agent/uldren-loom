package ai.uldren.loom;

import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;
import java.lang.invoke.MethodHandle;

public final class LanesOps {
    private static final MethodHandle CREATE = down("loom_lanes_create_cbor",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.JAVA_LONG, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle GET = down("loom_lanes_get_cbor",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle LIST = down("loom_lanes_list_cbor",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle UPDATE = down("loom_lanes_update_cbor",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private static final MethodHandle TICKET_ADD = down("loom_lanes_ticket_add_cbor",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS));
    private static final MethodHandle TICKET_REMOVE = down("loom_lanes_ticket_remove_cbor",
            FunctionDescriptor.of(ValueLayout.JAVA_INT, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS, ValueLayout.ADDRESS,
                    ValueLayout.ADDRESS, ValueLayout.ADDRESS));
    private final LoomSession session;

    LanesOps(LoomSession session) {
        this.session = session;
    }

    private static MethodHandle down(String symbol, FunctionDescriptor descriptor) {
        return Loom.LINKER.downcallHandle(Loom.LOOKUP.find(symbol).orElseThrow(), descriptor);
    }

    public byte[] create(String workspace, byte[] lane) {
        return session.onHandle("loom_lanes_create_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment bytes = Loom.bytesOrNull(arena, lane);
            int status = (int) CREATE.invokeExact(handle, arena.allocateFrom(workspace), bytes,
                    (long) lane.length, outPtr, outLen);
            return takeBytes("loom_lanes_create_cbor", status, outPtr, outLen);
        });
    }

    public byte[] get(String workspace, String laneId) {
        return session.onHandle("loom_lanes_get_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            MemorySegment found = arena.allocate(ValueLayout.JAVA_INT);
            int status = (int) GET.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(laneId), outPtr, outLen, found);
            if (status != 0) {
                throw Loom.lastError("loom_lanes_get_cbor");
            }
            if (found.get(ValueLayout.JAVA_INT, 0) == 0) {
                return null;
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] list(String workspace) {
        return bytes1("loom_lanes_list_cbor", LIST, workspace);
    }

    public byte[] update(String workspace, String laneId, String title, String description,
            String laneStatus, String statusReport, String reviewerFeedback, String updatedBy) {
        return session.onHandle("loom_lanes_update_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) UPDATE.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(laneId), nullable(arena, title), nullable(arena, description),
                    nullable(arena, laneStatus), nullable(arena, statusReport),
                    nullable(arena, reviewerFeedback), arena.allocateFrom(updatedBy), outPtr, outLen);
            return takeBytes("loom_lanes_update_cbor", status, outPtr, outLen);
        });
    }

    public byte[] ticketAdd(String workspace, String laneId, String ticketId, String updatedBy,
            String placement, String anchor) {
        return session.onHandle("loom_lanes_ticket_add_cbor", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) TICKET_ADD.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(laneId), arena.allocateFrom(ticketId),
                    arena.allocateFrom(updatedBy), nullable(arena, placement),
                    nullable(arena, anchor), outPtr, outLen);
            return takeBytes("loom_lanes_ticket_add_cbor", status, outPtr, outLen);
        });
    }

    public byte[] ticketRemove(String workspace, String laneId, String ticketId, String updatedBy) {
        return bytes4("loom_lanes_ticket_remove_cbor", TICKET_REMOVE, workspace, laneId, ticketId,
                updatedBy);
    }

    private byte[] bytes1(String symbol, MethodHandle method, String workspace) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(workspace), outPtr,
                    outLen);
            return takeBytes(symbol, status, outPtr, outLen);
        });
    }

    private byte[] bytes4(String symbol, MethodHandle method, String workspace, String laneId,
            String value, String updatedBy) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) method.invokeExact(handle, arena.allocateFrom(workspace),
                    arena.allocateFrom(laneId), arena.allocateFrom(value),
                    arena.allocateFrom(updatedBy), outPtr, outLen);
            return takeBytes(symbol, status, outPtr, outLen);
        });
    }

    private static MemorySegment nullable(java.lang.foreign.Arena arena, String value) {
        return value != null && !value.isEmpty() ? arena.allocateFrom(value) : MemorySegment.NULL;
    }

    private static byte[] takeBytes(String symbol, int status, MemorySegment outPtr,
            MemorySegment outLen) throws Throwable {
        if (status != 0) {
            throw Loom.lastError(symbol);
        }
        return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                outLen.get(ValueLayout.JAVA_LONG, 0));
    }
}
