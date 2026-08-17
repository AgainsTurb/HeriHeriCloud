plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

val gstreamerRoot = providers.gradleProperty("HERI_GSTREAMER_ANDROID_ROOT")
    .orElse(providers.environmentVariable("GSTREAMER_ROOT_ANDROID"))
val androidNdkVersion = providers.environmentVariable("HERI_ANDROID_NDK_VERSION")
    .orElse("29.0.13846066")
val localGstreamerPointer = file("../../../.gstreamer/android/current-root.txt")
val configuredGstreamerPath = gstreamerRoot.orNull
    ?: localGstreamerPointer.takeIf { it.isFile }?.readText()?.trim()
    ?: error("GSTREAMER_ROOT_ANDROID must point to the extracted universal Android SDK")
val configuredGstreamerRoot = file(configuredGstreamerPath)
val gstreamerRootPath = sequenceOf(configuredGstreamerRoot, configuredGstreamerRoot.parentFile)
    .filterNotNull()
    .firstOrNull { root ->
        listOf("arm64", "armv7", "x86", "x86_64").all { architecture ->
            root.resolve("$architecture/share/gst-android/ndk-build/gstreamer-1.0.mk").isFile
        }
    }?.absolutePath
    ?: error("GSTREAMER_ROOT_ANDROID does not contain the complete universal Android SDK")

android {
    namespace = "com.heriheri.gstreamerplayer"
    compileSdk = 36
    ndkVersion = androidNdkVersion.get()

    sourceSets.getByName("main").java.srcDir(
        "$gstreamerRootPath/arm64/share/gst-android/ndk-build"
    )

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")

        externalNativeBuild {
            ndkBuild {
                arguments("GSTREAMER_ROOT_ANDROID=$gstreamerRootPath")
                abiFilters("arm64-v8a", "x86_64")
            }
        }
    }

    externalNativeBuild {
        ndkBuild {
            path = file("src/main/jni/Android.mk")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation(project(":tauri-android"))
}
