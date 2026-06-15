// Kotlin Multiplatform binding over the Uldren Loom C ABI via JNI (jvm + androidTarget).
// Plugin versions below are starting points; align with your toolchain.
plugins {
    kotlin("multiplatform") version "2.1.0"
    id("com.android.library") version "8.7.2"
}

group = "ai.uldren"
version = "0.0.0"

kotlin {
    jvm()
    androidTarget()
}

android {
    namespace = "ai.uldren.loom"
    compileSdk = 35
    defaultConfig {
        minSdk = 24
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }
    // Builds the JNI shim per ABI; CMakeLists.txt links the Rust static lib (see README.md).
    externalNativeBuild {
        cmake { path = file("CMakeLists.txt") }
    }
}
