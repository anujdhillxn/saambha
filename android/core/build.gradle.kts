plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.saambha.core"
    compileSdk = 34

    defaultConfig {
        minSdk = 26
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }

    // The server's local CA (../server/ca.pem, made by scripts/gen_certs.sh) is bundled as res/raw/saambha_ca.
    // The apps trust only this CA when talking to the server.
    sourceSets["main"].res.srcDir(layout.buildDirectory.dir("generated/ca-res"))
}

val copyCa by tasks.registering(Copy::class) {
    from(rootProject.file("../server/ca.pem")) { rename { "saambha_ca.pem" } }
    into(layout.buildDirectory.dir("generated/ca-res/raw"))
}
tasks.named("preBuild") { dependsOn(copyCa) }

dependencies {
    api("androidx.swiperefreshlayout:swiperefreshlayout:1.1.0")
    implementation("com.squareup.okhttp3:okhttp:4.12.0")  // the touchpad's WebSocket
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")  // the real one; android.jar only has stubs
}
