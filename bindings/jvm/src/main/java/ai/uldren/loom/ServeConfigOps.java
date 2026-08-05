package ai.uldren.loom;

import java.lang.foreign.Arena;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.ValueLayout;

public final class ServeConfigOps {
    private final LoomSession session;

    ServeConfigOps(LoomSession session) {
        this.session = session;
    }

    public String listenerConfigureJson(String requestJson) {
        return stringCall("loom_serve_listener_configure_json", (arena, handle, out) ->
                (int) Loom.LOOM_SERVE_LISTENER_CONFIGURE_JSON.invokeExact(
                        handle, arena.allocateFrom(requestJson), out));
    }

    public String listenerListJson() {
        return stringCall("loom_serve_listener_list_json", (arena, handle, out) ->
                (int) Loom.LOOM_SERVE_LISTENER_LIST_JSON.invokeExact(handle, out));
    }

    public String listenerSetEnabledJson(String listenerId, boolean enabled) {
        return stringCall("loom_serve_listener_set_enabled_json", (arena, handle, out) ->
                (int) Loom.LOOM_SERVE_LISTENER_SET_ENABLED_JSON.invokeExact(
                        handle, arena.allocateFrom(listenerId), enabled ? 1 : 0, out));
    }

    public String listenerRemoveJson(String listenerId) {
        return stringCall("loom_serve_listener_remove_json", (arena, handle, out) ->
                (int) Loom.LOOM_SERVE_LISTENER_REMOVE_JSON.invokeExact(
                        handle, arena.allocateFrom(listenerId), out));
    }

    public String webRouteListJson(String listenerId) {
        return stringCall("loom_serve_web_route_list_json", (arena, handle, out) ->
                (int) Loom.LOOM_SERVE_WEB_ROUTE_LIST_JSON.invokeExact(
                        handle, arena.allocateFrom(listenerId), out));
    }

    public String webRouteSetJson(String requestJson) {
        return stringCall("loom_serve_web_route_set_json", (arena, handle, out) ->
                (int) Loom.LOOM_SERVE_WEB_ROUTE_SET_JSON.invokeExact(
                        handle, arena.allocateFrom(requestJson), out));
    }

    public String webRouteRemoveJson(String listenerId, String routeId) {
        return stringCall("loom_serve_web_route_remove_json", (arena, handle, out) ->
                (int) Loom.LOOM_SERVE_WEB_ROUTE_REMOVE_JSON.invokeExact(
                        handle, arena.allocateFrom(listenerId), arena.allocateFrom(routeId), out));
    }

    private String stringCall(String symbol, StringInvocation invocation) {
        return session.onHandle(symbol, (arena, handle) -> {
            MemorySegment out = arena.allocate(ValueLayout.ADDRESS);
            int status = invocation.invoke(arena, handle, out);
            if (status != 0) {
                throw Loom.lastError(symbol);
            }
            return Loom.takeOwnedString(out.get(ValueLayout.ADDRESS, 0));
        });
    }

    @FunctionalInterface
    private interface StringInvocation {
        int invoke(Arena arena, MemorySegment handle, MemorySegment out) throws Throwable;
    }
}
