plugins {
    `java-library`
}

group = "ai.uldren"
version = "0.1.0-alpha.1"

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(22)
        vendor = JvmVendorSpec.ADOPTIUM
    }
}

repositories { mavenCentral() }

dependencies {
    testImplementation("org.junit.jupiter:junit-jupiter:5.10.3")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

tasks.withType<JavaCompile>().configureEach {
    options.compilerArgs.add("-Xlint:all,-restricted")
}

tasks.test {
    failOnNoDiscoveredTests = false
    useJUnitPlatform()
    jvmArgs("--enable-native-access=ALL-UNNAMED")
}

val nativeLibraryDir = file("../../target/release").absolutePath
val focusedNativeLibraryDir = file("../../target/debug").absolutePath

val runtimeSmoke by tasks.registering(JavaExec::class) {
    dependsOn(tasks.testClasses)
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("ai.uldren.loom.LoomRuntimeSmoke")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "-Djava.library.path=$nativeLibraryDir")
    environment("LD_LIBRARY_PATH", listOfNotNull(nativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(nativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}

val operationalRuntimeSmoke by tasks.registering(JavaExec::class) {
    dependsOn(tasks.testClasses)
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("ai.uldren.loom.LoomRuntimeSmoke")
    args("operational")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "-Djava.library.path=$focusedNativeLibraryDir")
    environment("LD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}

val interchangeRuntimeSmoke by tasks.registering(JavaExec::class) {
    dependsOn(tasks.testClasses)
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("ai.uldren.loom.LoomRuntimeSmoke")
    args("interchange")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "-Djava.library.path=$focusedNativeLibraryDir")
    environment("LD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}

val dataExecutionRuntimeSmoke by tasks.registering(JavaExec::class) {
    dependsOn(tasks.testClasses)
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("ai.uldren.loom.LoomRuntimeSmoke")
    args("data-execution")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "-Djava.library.path=$focusedNativeLibraryDir")
    environment("LD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}

val driveRuntimeSmoke by tasks.registering(JavaExec::class) {
    dependsOn(tasks.testClasses)
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("ai.uldren.loom.LoomRuntimeSmoke")
    args("drive")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "-Djava.library.path=$focusedNativeLibraryDir")
    environment("LD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}

val chatRuntimeSmoke by tasks.registering(JavaExec::class) {
    dependsOn(tasks.testClasses)
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("ai.uldren.loom.LoomRuntimeSmoke")
    args("chat")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "-Djava.library.path=$focusedNativeLibraryDir")
    environment("LD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(focusedNativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}

tasks.check {
    dependsOn(runtimeSmoke)
}
