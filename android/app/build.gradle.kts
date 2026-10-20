import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.google.services)
}

// The backend's address and the app key belong to each user's own build: they
// live in local.properties, out of the repository. Without them the app builds
// but says it is not configured.
val localProperties = Properties().apply {
    providers.fileContents(rootProject.layout.projectDirectory.file("local.properties")).asText.orNull
        ?.let { load(it.reader()) }
}

fun quoted(name: String) = "\"" + localProperties.getProperty(name, "") + "\""

android {
    namespace = "io.github.nonock.eztravel"
    compileSdk {
        version = release(37)
    }

    defaultConfig {
        applicationId = "io.github.nonock.eztravel"
        // Android 8.0, for java.time.
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "1.0"

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        buildConfigField("String", "BACKEND_URL", quoted("eztravel.backendUrl"))
        buildConfigField("String", "APP_KEY", quoted("eztravel.appKey"))
    }

    buildTypes {
        debug {
            enableUnitTestCoverage = true
            enableAndroidTestCoverage = true
        }
        release {
            optimization {
                enable = true
                packageScope = setOf("androidx.**", "kotlin.**", "kotlinx.**")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    lint {
        // Every text shown to the user must exist in French too.
        error += "MissingTranslation"
    }
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.core)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.core.splashscreen)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(platform(libs.firebase.bom))
    implementation(libs.firebase.messaging)
    // Firebase pulls an old Fragment that breaks registerForActivityResult.
    implementation(libs.androidx.fragment)
    testImplementation(libs.junit)
    testImplementation(libs.org.json)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.espresso.core)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.test.rules)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
    debugImplementation(libs.androidx.compose.ui.tooling)
}