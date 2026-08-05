package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class StoreAdminOps {
    private final LoomSession session;

    StoreAdminOps(LoomSession session) {
        this.session = session;
    }

    public byte[] bundleImport(byte[] bundle, boolean dryRun) {
        return session.onHandle("loom_store_bundle_import", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_STORE_BUNDLE_IMPORT.invokeExact(
                    handle, Loom.bytesOrNull(arena, bundle), (long) (bundle != null ? bundle.length : 0),
                    dryRun ? 1 : 0, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_store_bundle_import");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }

    public byte[] maintenanceStatus(byte[] request) {
        return generatedBytes("loom_store_maintenance_status", Loom.LOOM_STORE_MAINTENANCE_STATUS, request);
    }

    public byte[] maintenancePolicySet(byte[] update) {
        return generatedBytes("loom_store_maintenance_policy_set", Loom.LOOM_STORE_MAINTENANCE_POLICY_SET, update);
    }

    public byte[] maintenanceRun(byte[] request) {
        return generatedBytes("loom_store_maintenance_run", Loom.LOOM_STORE_MAINTENANCE_RUN, request);
    }

    private byte[] generatedBytes(String symbol, java.lang.invoke.MethodHandle method, byte[] request) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) method.invokeExact(
                    handle, Loom.bytesOrNull(arena, request), (long) (request != null ? request.length : 0),
                    outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError(symbol);
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }
}
