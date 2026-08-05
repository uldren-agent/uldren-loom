package ai.uldren.loom;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;

import org.junit.jupiter.api.Test;

final class AuditOpsTest {
    @Test
    void mu6iD7AuditCompactIsPublicBytesWrapperWithGeneratedSymbol() throws Exception {
        Method method = AuditOps.class.getMethod("compact", long.class);
        assertArrayEquals(new Class<?>[] { long.class }, method.getParameterTypes());
        assertTrue(method.getReturnType().equals(byte[].class));

        String loomSource = Files.readString(Path.of("src/main/java/ai/uldren/loom/Loom.java"));
        assertTrue(loomSource.contains("LOOKUP.find(\"loom_audit_compact\")"));
        assertTrue(loomSource.contains("LOOM_AUDIT_COMPACT"));

        String opsSource = Files.readString(Path.of("src/main/java/ai/uldren/loom/AuditOps.java"));
        assertTrue(opsSource.contains("Loom.LOOM_AUDIT_COMPACT.invokeExact"));
        assertTrue(opsSource.contains("Loom.takeOwnedBytes"));
        assertTrue(opsSource.contains("throughSeq < 0"));
    }

    @Test
    void mu6iD7AuditCompactRejectsNegativeSequenceBeforeUnsignedForwarding() {
        AuditOps audit = new LoomSession("/tmp/mu6i-d7-unused.loom", null).audit();
        assertThrows(IllegalArgumentException.class, () -> audit.compact(-1));
    }
}
