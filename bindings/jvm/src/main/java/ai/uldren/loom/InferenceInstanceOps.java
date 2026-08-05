package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class InferenceInstanceOps {
    private final LoomSession session;

    InferenceInstanceOps(LoomSession session) {
        this.session = session;
    }

    public String createJson(String workspace, String name, String model, String kind,
                             String runtime, String preset, String settingsJson) {
        return session.onHandle("loom_inference_instance_create_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_INFERENCE_INSTANCE_CREATE_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(name),
                    arena.allocateFrom(model), arena.allocateFrom(kind), arena.allocateFrom(runtime),
                    nullable(arena, preset), nullable(arena, settingsJson), out);
            if (status != 0) {
                throw Loom.lastError("loom_inference_instance_create_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    public String updateJson(String workspace, String name, String preset, String settingsJson) {
        return session.onHandle("loom_inference_instance_update_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_INFERENCE_INSTANCE_UPDATE_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(name),
                    nullable(arena, preset), nullable(arena, settingsJson), out);
            if (status != 0) {
                throw Loom.lastError("loom_inference_instance_update_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    public String deleteJson(String workspace, String name) {
        return session.onHandle("loom_inference_instance_delete_json", (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = (int) Loom.LOOM_INFERENCE_INSTANCE_DELETE_JSON.invokeExact(
                    handle, arena.allocateFrom(workspace), arena.allocateFrom(name), out);
            if (status != 0) {
                throw Loom.lastError("loom_inference_instance_delete_json");
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    private static MemorySegment nullable(Arena arena, String value) {
        return value != null ? arena.allocateFrom(value) : MemorySegment.NULL;
    }
}
