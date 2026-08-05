package ai.uldren.loom;

import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class LifecycleOps {
    private final LoomSession session;

    LifecycleOps(LoomSession session) {
        this.session = session;
    }

    public String defineStandardJson(String workspace, String kind, String version,
                                     String completionPredicateDigest) {
        return session.onHandle("loom_lifecycle_define_standard_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_LIFECYCLE_DEFINE_STANDARD_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(kind),
                    arena.allocateFrom(version), arena.allocateFrom(completionPredicateDigest), out);
            if (status != 0) {
                throw Loom.lastError("loom_lifecycle_define_standard_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    public String defineJson(String workspace, byte[] definition) {
        return session.onHandle("loom_lifecycle_define_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_LIFECYCLE_DEFINE_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), Loom.bytesOrNull(arena, definition),
                    (long) (definition != null ? definition.length : 0), out);
            if (status != 0) {
                throw Loom.lastError("loom_lifecycle_define_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    public String instantiateJson(String workspace, String instanceId, String definitionId,
                                  String subjectRefsJson) {
        return session.onHandle("loom_lifecycle_instantiate_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_LIFECYCLE_INSTANTIATE_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(instanceId),
                    arena.allocateFrom(definitionId), arena.allocateFrom(subjectRefsJson), out);
            if (status != 0) {
                throw Loom.lastError("loom_lifecycle_instantiate_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    public String transitionJson(String workspace, String instanceId, String transitionId,
                                 String toStageId, String actorPrincipalId,
                                 String gateEvaluationsJson, String snapshotDigest) {
        return session.onHandle("loom_lifecycle_transition_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_LIFECYCLE_TRANSITION_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(instanceId),
                    arena.allocateFrom(transitionId), arena.allocateFrom(toStageId),
                    actorPrincipalId != null ? arena.allocateFrom(actorPrincipalId) : MemorySegment.NULL,
                    arena.allocateFrom(gateEvaluationsJson),
                    snapshotDigest != null ? arena.allocateFrom(snapshotDigest) : MemorySegment.NULL,
                    out);
            if (status != 0) {
                throw Loom.lastError("loom_lifecycle_transition_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }
}
