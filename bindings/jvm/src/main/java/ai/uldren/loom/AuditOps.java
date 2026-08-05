package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class AuditOps {
    private final LoomSession session;

    AuditOps(LoomSession session) {
        this.session = session;
    }

    public byte[] compact(long throughSeq) {
        if (throughSeq < 0) {
            throw new IllegalArgumentException("throughSeq must be non-negative");
        }
        return session.onHandle("loom_audit_compact", (arena, handle) -> {
            MemorySegment outPtr = arena.allocate(ValueLayout.ADDRESS);
            MemorySegment outLen = arena.allocate(ValueLayout.JAVA_LONG);
            int status = (int) Loom.LOOM_AUDIT_COMPACT.invokeExact(
                    handle, throughSeq, outPtr, outLen);
            if (status != 0) {
                throw Loom.lastError("loom_audit_compact");
            }
            return Loom.takeOwnedBytes(outPtr.get(ValueLayout.ADDRESS, 0),
                    outLen.get(ValueLayout.JAVA_LONG, 0));
        });
    }
}
