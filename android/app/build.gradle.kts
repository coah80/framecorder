import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "com.framecorder.app"
    compileSdk = 37

    defaultConfig {
        // the same id as the old Tauri app, so this one replaces it
        applicationId = "com.framecorder.app"
        minSdk = 29
        targetSdk = 37
        // major * 1_000_000 + minor * 1000 + patch, like Tauri counted
        versionCode = 2000
        versionName = "0.2.0"
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    // a real key from keystore.properties when there is one; otherwise the
    // debug key, like the old app's "debugsigned" release, so it still
    // installs over it when built on the same machine
    val keystore = rootProject.file("keystore.properties")
    signingConfigs {
        if (keystore.exists()) {
            create("release") {
                val p = Properties().apply { keystore.inputStream().use { load(it) } }
                storeFile = rootProject.file(p.getProperty("storeFile"))
                storePassword = p.getProperty("storePassword")
                keyAlias = p.getProperty("keyAlias")
                keyPassword = p.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        named("main") {
            kotlin.directories.add(layout.buildDirectory.dir("generated/uniffi").get().asFile.path)
            jniLibs.directories.add(layout.buildDirectory.dir("rustJniLibs").get().asFile.path)
        }
    }

    packaging {
        // the desktop app's crate builds its own .so too; only the FFI one is used
        jniLibs.excludes += "**/libframecorder_app_lib.so"
    }
}

kotlin {
    compilerOptions {
        optIn.addAll(
            "androidx.compose.material3.ExperimentalMaterial3Api",
            "androidx.compose.material3.ExperimentalMaterial3ExpressiveApi",
            "androidx.compose.animation.ExperimentalSharedTransitionApi",
            "androidx.compose.foundation.layout.ExperimentalLayoutApi",
        )
    }
}

// The sync core is Rust (../rust, which uses ../../app/src/core). Gradle
// builds it with cargo-ndk and writes its Kotlin bindings with UniFFI.
// -PskipRust uses whatever's already in build/.
val rustDir = rootProject.layout.projectDirectory.dir("rust")
val rustInputs = files(
    fileTree(rustDir) { include("**/*.rs", "**/Cargo.toml", "**/uniffi.toml", "Cargo.lock"); exclude("target/**") },
    fileTree(rootProject.file("../app/src/core")),
    rootProject.file("../app/Cargo.toml"),
)
val skipRust = providers.gradleProperty("skipRust").isPresent

val cargoNdk = tasks.register<Exec>("cargoNdk") {
    group = "rust"
    description = "Builds the sync core for the phone's CPUs"
    enabled = !skipRust
    workingDir(rustDir)
    inputs.files(rustInputs)
    val out = layout.buildDirectory.dir("rustJniLibs")
    outputs.dir(out)
    commandLine(
        "cargo", "ndk", "-t", "arm64-v8a", "-t", "x86_64", "--platform", "29",
        "-o", out.get().asFile.absolutePath,
        "build", "-p", "framecorder-ffi", "--release",
    )
}

// a host build, since the phone's release library is stripped of the
// metadata the bindings are written from
val hostCore = tasks.register<Exec>("hostCore") {
    group = "rust"
    enabled = !skipRust
    workingDir(rustDir)
    inputs.files(rustInputs)
    outputs.dir(rustDir.dir("target/debug"))
    commandLine("cargo", "build", "-q", "-p", "framecorder-ffi")
}

val uniffiBindings = tasks.register<Exec>("uniffiBindings") {
    group = "rust"
    description = "Writes the Kotlin side of the sync core"
    enabled = !skipRust
    dependsOn(hostCore)
    workingDir(rustDir)
    inputs.files(rustInputs)
    val out = layout.buildDirectory.dir("generated/uniffi")
    outputs.dir(out)
    val os = System.getProperty("os.name").lowercase()
    val lib = when {
        os.contains("win") -> "framecorder_ffi.dll"
        os.contains("mac") -> "libframecorder_ffi.dylib"
        else -> "libframecorder_ffi.so"
    }
    commandLine(
        "cargo", "run", "-q", "-p", "uniffi-bindgen", "--",
        "generate", "--library", rustDir.file("target/debug/$lib").asFile.absolutePath,
        "--language", "kotlin", "--out-dir", out.get().asFile.absolutePath, "--no-format",
    )
}

tasks.named("preBuild") { dependsOn(cargoNdk, uniffiBindings) }

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.animation)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.material3)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.lifecycle.process)
    implementation(libs.lifecycle.service)
    implementation(libs.navigation3.runtime)
    implementation(libs.navigation3.ui)
    implementation(libs.media3.exoplayer)
    implementation(libs.media3.ui.compose)
    implementation(libs.media3.transformer)
    implementation(libs.haze)
    implementation(libs.haze.blur)
    implementation(libs.core.ktx)
    implementation(libs.coroutines.android)
    implementation(libs.code.scanner)
    implementation("${libs.jna.get()}@aar")
    debugImplementation(libs.compose.ui.tooling)
}
