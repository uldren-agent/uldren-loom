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

tasks.withType<JavaCompile>().configureEach {
    options.compilerArgs.add("-Xlint:all,-restricted")
}

tasks.test {
    failOnNoDiscoveredTests = false
    jvmArgs("--enable-native-access=ALL-UNNAMED")
}

val nativeLibraryDir = file("../../target/release").absolutePath

val runtimeSmoke by tasks.registering(JavaExec::class) {
    dependsOn(tasks.testClasses)
    classpath = sourceSets["test"].runtimeClasspath
    mainClass.set("ai.uldren.loom.LoomRuntimeSmoke")
    jvmArgs("--enable-native-access=ALL-UNNAMED", "-Djava.library.path=$nativeLibraryDir")
    environment("LD_LIBRARY_PATH", listOfNotNull(nativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(nativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}

tasks.check {
    dependsOn(runtimeSmoke)
}
