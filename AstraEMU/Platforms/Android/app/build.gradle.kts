import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

fun requiredEnvironment(name: String): String =
    providers.environmentVariable(name).orNull
        ?: throw GradleException("$name is required for AstraEMU release packaging")

val generatedRoot = layout.projectDirectory.dir("../generated")

android {
    namespace = "org.astraemu.manager"
    compileSdk = 36

    defaultConfig {
        applicationId = "org.astraemu.manager"
        minSdk = 26
        targetSdk = 36
        versionCode = providers.environmentVariable("ASTRA_EMU_ANDROID_VERSION_CODE")
            .orElse("1")
            .get()
            .toInt()
        versionName = providers.environmentVariable("ASTRA_EMU_ANDROID_VERSION_NAME")
            .orElse("0.1.0")
            .get()
        ndk {
            abiFilters += setOf("arm64-v8a", "x86_64")
        }
    }

    signingConfigs {
        create("release") {
            storeFile = file(requiredEnvironment("ASTRA_EMU_ANDROID_KEYSTORE"))
            storePassword = requiredEnvironment("ASTRA_EMU_ANDROID_KEYSTORE_PASSWORD")
            keyAlias = requiredEnvironment("ASTRA_EMU_ANDROID_KEY_ALIAS")
            keyPassword = requiredEnvironment("ASTRA_EMU_ANDROID_KEY_PASSWORD")
            enableV1Signing = true
            enableV2Signing = true
            enableV3Signing = true
            enableV4Signing = true
        }
    }

    buildTypes {
        debug {
            isDebuggable = true
        }
        release {
            isMinifyEnabled = false
            isDebuggable = false
            signingConfig = signingConfigs.getByName("release")
        }
    }

    sourceSets.getByName("main") {
        jniLibs.srcDir(generatedRoot.dir("jniLibs"))
        assets.srcDir(generatedRoot.dir("assets"))
    }

    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }
}

kotlin {
    jvmToolchain(17)
}
