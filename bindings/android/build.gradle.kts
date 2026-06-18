plugins {
    kotlin("multiplatform") version "2.1.0"
    id("com.android.library") version "8.7.2"
}

group = "ai.uldren"
version = "0.1.0-alpha.1"

kotlin {
    jvmToolchain(17)
    compilerOptions {
        freeCompilerArgs.add("-Xexpect-actual-classes")
    }
    jvm()
    androidTarget()
    sourceSets {
        commonTest.dependencies {
            implementation(kotlin("test"))
        }
    }
}

android {
    namespace = "ai.uldren.loom"
    compileSdk = 35
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    defaultConfig {
        minSdk = 24
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }
    externalNativeBuild {
        cmake { path = file("CMakeLists.txt") }
    }
}

val nativeLibraryDir = file("../../target/release").absolutePath
val hostJniBuildDir = layout.buildDirectory.dir("host-jni")

val configureHostJni by tasks.registering(Exec::class) {
    inputs.file("CMakeLists.txt")
    inputs.file("native/uldren_loom_jni.c")
    inputs.dir("../../include")
    inputs.dir("../../target/release")
    outputs.dir(hostJniBuildDir)
    commandLine(
        "cmake",
        "-S",
        projectDir.absolutePath,
        "-B",
        hostJniBuildDir.get().asFile.absolutePath,
        "-DLOOM_NATIVE_DIR=$nativeLibraryDir",
    )
}

val buildHostJni by tasks.registering(Exec::class) {
    dependsOn(configureHostJni)
    commandLine("cmake", "--build", hostJniBuildDir.get().asFile.absolutePath)
}

tasks.named<Test>("jvmTest") {
    dependsOn(buildHostJni)
    val hostJniDir = hostJniBuildDir.get().asFile.absolutePath
    systemProperty("java.library.path", listOf(hostJniDir, nativeLibraryDir).joinToString(":"))
    environment("LD_LIBRARY_PATH", listOfNotNull(hostJniDir, nativeLibraryDir, System.getenv("LD_LIBRARY_PATH")).joinToString(":"))
    environment("DYLD_LIBRARY_PATH", listOfNotNull(hostJniDir, nativeLibraryDir, System.getenv("DYLD_LIBRARY_PATH")).joinToString(":"))
}
