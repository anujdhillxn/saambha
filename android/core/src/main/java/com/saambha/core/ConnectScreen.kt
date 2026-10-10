package com.saambha.core

import android.app.Activity
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView
import androidx.swiperefreshlayout.widget.SwipeRefreshLayout
import kotlin.concurrent.thread

/**
 * Covers the app's [content] until it's connected to the server: only a spinner (plus a line
 * while the desktop has to approve), or the error with pull-to-retry if connecting fails.
 * Add [view] on top of [content] (e.g. both in a FrameLayout), then call [connect].
 */
class ConnectScreen(
    private val activity: Activity,
    private val saambha: SaambhaClient,
    private val content: View,
    private val spinner: ProgressBar,
    private val message: TextView,
    sidePaddingPx: Int,
    private val onConnected: (serverName: String) -> Unit,
    private val onFailed: () -> Unit = {},
) {
    var connected = false
        private set
    private var connecting = false

    /** Pulling down pulls out the animated indicator; releasing past the threshold retries. */
    val view: SwipeRefreshLayout = SwipeRefreshLayout(activity).apply {
        setColorSchemeColors(0xFFC0392B.toInt())
        setProgressBackgroundColorSchemeColor(0xFF2A2A2A.toInt())
        addView(LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(sidePaddingPx, 0, sidePaddingPx, 0)
            addView(spinner)
            addView(message)
        }, MATCH_PARENT, MATCH_PARENT)
        isEnabled = false
        setOnRefreshListener { connect(fromPull = true) }
    }

    /** Connects in the background (scan + pair if needed), showing the spinner meanwhile. */
    fun connect() = connect(fromPull = false)

    /** [fromPull]: started by pull-to-retry, so its indicator is already spinning at the top. */
    private fun connect(fromPull: Boolean) {
        if (connecting) return
        connecting = true
        if (!fromPull) showLoading("")
        activity.window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        thread {
            val result = runCatching {
                saambha.connect(onAwaitingApproval = { name -> activity.runOnUiThread { showLoading("Approve on $name…") } })
            }
            activity.runOnUiThread {
                connecting = false
                view.isRefreshing = false
                activity.window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                result.onSuccess { name ->
                    connected = true
                    view.visibility = View.GONE
                    content.visibility = View.VISIBLE
                    onConnected(name)
                }
                result.onFailure {
                    showFailed(it.message ?: "Couldn't connect")
                    onFailed()
                }
            }
        }
    }

    private fun showLoading(text: String) {
        content.visibility = View.GONE
        view.visibility = View.VISIBLE
        view.isRefreshing = false
        view.isEnabled = false
        spinner.visibility = View.VISIBLE
        message.text = text
    }

    private fun showFailed(text: String) {
        content.visibility = View.GONE
        view.visibility = View.VISIBLE
        spinner.visibility = View.GONE
        message.text = "$text\n\nPull down to retry"
        view.isEnabled = true
    }
}
