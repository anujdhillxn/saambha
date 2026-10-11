package com.saambha.core

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.InputDevice
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.ProgressBar
import android.widget.TextView
import android.widget.Toast
import kotlin.concurrent.thread

/**
 * The touchpad: the whole screen moves the desktop's mouse (see [Touchpad] for how). Connects like
 * the main screen; leaving it lets go of the mouse.
 */
class MouseActivity : Activity() {

    private val settings = Settings()
    private lateinit var saambha: SaambhaClient
    private lateinit var connectScreen: ConnectScreen
    private lateinit var pad: FrameLayout
    private var mouse: MouseConnection? = null

    // Keeps Wi-Fi from saving power between packets, which holds the moves back and sends them in bunches.
    @Suppress("DEPRECATION")  // WIFI_MODE_FULL_HIGH_PERF: what's left below Android 10
    private val fastWifi by lazy {
        getSystemService(WifiManager::class.java).createWifiLock(
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            else WifiManager.WIFI_MODE_FULL_HIGH_PERF,
            "saambha:touchpad",
        )
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        saambha = Saambha.create(this, settings)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        fastWifi.acquire()
        setContentView(buildLayout())
        connectScreen.connect()
    }

    override fun onStop() {
        super.onStop()
        finish()
    }

    override fun onDestroy() {
        mouse?.close()
        fastWifi.release()
        super.onDestroy()
    }

    private fun openMouse() = thread {
        val result = runCatching {
            saambha.openMouse(onClosed = { reason -> runOnUiThread { leave(reason) } })
        }
        runOnUiThread {
            result.onSuccess { opened ->
                if (isFinishing) opened.close() else {
                    mouse = opened
                    pad.addView(TouchpadView(this, Touchpad(opened, settings)), MATCH_PARENT, MATCH_PARENT)
                }
            }
            result.onFailure { leave(it.message ?: "Couldn't reach Saambha") }
        }
    }

    private fun leave(reason: String) {
        if (isFinishing) return
        Toast.makeText(this, reason, Toast.LENGTH_LONG).show()
        finish()
    }

    private fun buildLayout(): View {
        val watch = packageManager.hasSystemFeature(PackageManager.FEATURE_WATCH)
        val hint = TextView(this).apply {
            text = if (watch) "Slide to move, tap to click.\nHold to right-click.\nTurn the crown to scroll."
            else "Slide to move, tap to click.\nHold, or tap with two fingers, to right-click.\nSlide two fingers to scroll."
            setTextColor(Color.GRAY)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, if (watch) 13f else 15f)
            gravity = Gravity.CENTER
        }
        pad = FrameLayout(this).apply {
            visibility = View.GONE
            addView(hint, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        }
        connectScreen = ConnectScreen(
            this, saambha, pad,
            spinner = ProgressBar(this),
            message = TextView(this).apply {
                setTextColor(Color.LTGRAY)
                gravity = Gravity.CENTER
            },
            sidePaddingPx = (24 * resources.displayMetrics.density).toInt(),
            onConnected = { openMouse() },
        )
        return FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
            addView(pad, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
            addView(connectScreen.view, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        }
    }
}

/** Feeds the [touchpad] the touches on it (in dp) and the crown's turns, and buzzes on a click. */
@SuppressLint("ViewConstructor")
private class TouchpadView(context: Context, private val touchpad: Touchpad) : View(context) {

    private val density = resources.displayMetrics.density

    init {
        // The crown's turns go to the focused view.
        isFocusable = true
        isFocusableInTouchMode = true
        requestFocus()
    }

    @SuppressLint("ClickableViewAccessibility")  // a touchpad: it's all custom gestures
    override fun onTouchEvent(event: MotionEvent): Boolean {
        when (event.actionMasked) {
            MotionEvent.ACTION_UP -> touchpad.release(event.eventTime)?.let {
                performHapticFeedback(HapticFeedbackConstants.VIRTUAL_KEY)
            }
            MotionEvent.ACTION_CANCEL -> touchpad.cancel()
            else -> {
                // A finger lifting is still in the event: leave it out.
                val lifted = if (event.actionMasked == MotionEvent.ACTION_POINTER_UP) event.actionIndex else -1
                val fingers = (0 until event.pointerCount).filter { it != lifted }
                touchpad.touch(
                    fingers.size,
                    fingers.map { event.getX(it) }.average().toFloat() / density,
                    fingers.map { event.getY(it) }.average().toFloat() / density,
                    event.eventTime,
                )
            }
        }
        return true
    }

    override fun onGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.action != MotionEvent.ACTION_SCROLL || !event.isFromSource(InputDevice.SOURCE_ROTARY_ENCODER)) {
            return super.onGenericMotionEvent(event)
        }
        touchpad.turn(event.getAxisValue(MotionEvent.AXIS_SCROLL), event.eventTime)
        return true
    }
}
