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
    // The touchpad. The desktop speeds the pointer up further for fast slides.
    val pointerSpeed: Float = 2f,  // desktop pixels per dp a finger slides
    val scrollStepDp: Float = 24f,  // how far two fingers slide per notch of the wheel
    val crownNotchesPerTurn: Float = 1f,  // per unit of the crown's AXIS_SCROLL
    val tapSlopDp: Float = 8f,  // a touch that slides further isn't a tap
    val rightClickHoldMs: Long = 500,  // a tap held this long is a right click
    val mousePingSeconds: Long = 10,
)
