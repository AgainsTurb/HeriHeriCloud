import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val releaseKeystoreFile = rootProject.file("keystore.properties")
val releaseKeystoreProperties = Properties().apply {
    if (releaseKeystoreFile.exists()) {
        releaseKeystoreFile.inputStream().use { load(it) }
    }
}
val releaseKeyAlias = releaseKeystoreProperties.getProperty("keyAlias")
val releaseKeyPassword = releaseKeystoreProperties.getProperty("keyPassword")
    ?: releaseKeystoreProperties.getProperty("password")
val releaseStoreFile = releaseKeystoreProperties.getProperty("storeFile")
val releaseStorePassword = releaseKeystoreProperties.getProperty("storePassword")
    ?: releaseKeystoreProperties.getProperty("password")
val hasReleaseSigning = listOf(
    releaseKeyAlias,
    releaseKeyPassword,
    releaseStoreFile,
    releaseStorePassword,
).all { !it.isNullOrBlank() }
val androidNdkHome = providers.environmentVariable("ANDROID_NDK_HOME")
    .orElse(providers.environmentVariable("NDK_HOME"))

fun ndkHostTag(): String {
    val os = System.getProperty("os.name").lowercase()
    val arch = System.getProperty("os.arch").lowercase()
    return when {
        os.contains("win") -> "windows-x86_64"
        os.contains("mac") && (arch.contains("aarch64") || arch.contains("arm64")) -> "darwin-arm64"
        os.contains("mac") -> "darwin-x86_64"
        else -> "linux-x86_64"
    }
}

android {
    compileSdk = 36
    namespace = "com.alfred_whitman.heriheri"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.alfred_whitman.heriheri"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
                storeFile = file(releaseStoreFile!!)
                storePassword = releaseStorePassword
            }
        }
    }

    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            // Native symbols make the GStreamer and Rust libraries hundreds of MB larger.
            // Keep regular application debugging while allowing AGP to strip packaged JNI code.
            isJniDebuggable = false
            isMinifyEnabled = false
        }
        getByName("release") {
            if (hasReleaseSigning) {
                signingConfig = signingConfigs.getByName("release")
            }
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

fun stripNativeDebugSections(outputDirectory: File) {
    val ndkRoot = androidNdkHome.orNull
        ?: throw GradleException("ANDROID_NDK_HOME or NDK_HOME is required to strip native debug symbols")
    val stripTool = file("$ndkRoot/toolchains/llvm/prebuilt/${ndkHostTag()}/bin/llvm-strip" +
        if (System.getProperty("os.name").lowercase().contains("win")) ".exe" else "")
    check(stripTool.isFile) { "NDK llvm-strip was not found at ${stripTool.absolutePath}" }

    fileTree(outputDirectory).matching { include("**/*.so") }.files.forEach { library ->
        exec {
            commandLine(stripTool.absolutePath, "--strip-debug", library.absolutePath)
        }
    }
}

// AGP 8.11 on Windows can copy native libraries unchanged when its legacy strip
// executable is unavailable. Strip the merged package input first, then repeat on
// AGP's copied output as a defensive fallback for other plugin/native task paths.
tasks.configureEach {
    if (name.startsWith("merge") && name.endsWith("NativeLibs")) {
        doLast {
            val variant = name.removePrefix("merge").removeSuffix("NativeLibs")
                .replaceFirstChar { it.lowercase() }
            val outputDirectory = layout.buildDirectory.dir(
                "intermediates/merged_native_libs/$variant/$name/out/lib",
            ).get().asFile
            stripNativeDebugSections(outputDirectory)
        }
    }
    if (name.startsWith("strip") && name.endsWith("DebugSymbols")) {
        doLast {
            val variant = name.removePrefix("strip").removeSuffix("DebugSymbols")
                .replaceFirstChar { it.lowercase() }
            val outputDirectory = layout.buildDirectory.dir(
                "intermediates/stripped_native_libs/$variant/$name/out/lib",
            ).get().asFile
            stripNativeDebugSections(outputDirectory)
        }
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")
