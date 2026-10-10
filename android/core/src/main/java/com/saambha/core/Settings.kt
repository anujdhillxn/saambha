package com.saambha.core

/** Every tunable value for finding and talking to the saambha server, in one place. */
data class Settings(
    val port: Int = 8765,
    val scanThreads: Int = 48,
    val portTimeoutMs: Int = 400,
    val wifiTimeoutSeconds: Long = 8,
    val connectTimeoutMs: Int = 5_000,
    val readTimeoutMs: Int = 10_000,
    val discoverConnectTimeoutMs: Int = 2_000,
    val discoverReadTimeoutMs: Int = 3_000,
    val pairReadTimeoutMs: Int = 75_000,  // the desktop waits up to 60s for Allow
    val commandReadTimeoutMs: Int = 130_000,  // the server waits for Claude to finish (up to ~120s)
)
