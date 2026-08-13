plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

val gstreamerRoot = providers.gradleProperty("HERI_GSTREAMER_ANDROID_ROOT")
    .orElse(providers.environmentVariable("GSTREAMER_ROOT_ANDROID"))

android {
    namespace = "com.heriheri.gstreamerplayer"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")

        externalNativeBuild {
            ndkBuild {
                arguments("GSTREAMER_ROOT_ANDROID=${gstreamerRoot.orNull ?: ""}")
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
