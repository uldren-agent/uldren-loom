package ai.uldren.loom;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Map;

import org.junit.jupiter.api.Test;

final class StoreAdminOpsTest {
    @Test
    void mu6iD7StoreMaintenanceMethodsArePublicBytesWrappers() throws Exception {
        Map<String, String> methods = Map.of(
                "maintenanceStatus", "loom_store_maintenance_status",
                "maintenancePolicySet", "loom_store_maintenance_policy_set",
                "maintenanceRun", "loom_store_maintenance_run");
        for (String methodName : methods.keySet()) {
            Method method = StoreAdminOps.class.getMethod(methodName, byte[].class);
            assertArrayEquals(new Class<?>[] { byte[].class }, method.getParameterTypes());
            assertTrue(method.getReturnType().equals(byte[].class));
        }

        String loomSource = Files.readString(Path.of("src/main/java/ai/uldren/loom/Loom.java"));
        for (String symbol : methods.values()) {
            assertTrue(loomSource.contains("LOOKUP.find(\"" + symbol + "\")"));
        }

        String opsSource = Files.readString(Path.of("src/main/java/ai/uldren/loom/StoreAdminOps.java"));
        for (String symbol : methods.values()) {
            assertTrue(opsSource.contains("\"" + symbol + "\""));
        }
        assertTrue(opsSource.contains("Loom.bytesOrNull(arena, request)"));
        assertTrue(opsSource.contains("request != null ? request.length : 0"));
        assertTrue(opsSource.contains("Loom.takeOwnedBytes"));
    }
}
