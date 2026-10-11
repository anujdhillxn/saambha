package com.saambha.watch

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.os.VibrationEffect
import android.os.Vibrator
import android.speech.RecognizerIntent
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.WindowManager
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import com.saambha.core.ConnectScreen
import com.saambha.core.MouseActivity
import com.saambha.core.Saambha
import com.saambha.core.SaambhaClient
import kotlin.concurrent.thread

/**
 * Shows only a spinner while connecting (scan + pair if needed); pull down to retry if that fails.
 * Once connected it opens speech input, sends the words to saambha, and shows the reply.
 * Mouse opens the touchpad.
 */
class MainActivity : Activity() {

    private lateinit var saambha: SaambhaClient

    private lateinit var connectScreen: ConnectScreen
    private lateinit var status: TextView
    private lateinit var heard: TextView
    private lateinit var reply: TextView
    private lateinit var askButton: Button

    private var busy = false
    private var listenWhenConnected = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        saambha = Saambha.create(this)
        setContentView(buildLayout())
        listenWhenConnected = savedInstanceState == null
        connectScreen.connect()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // Tile tapped again while the app is open: start a new question.
        if (busy) return
        if (connectScreen.connected) startListening() else listenWhenConnected = true
    }

    private fun startListening() {
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
            .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            .putExtra(RecognizerIntent.EXTRA_PROMPT, "Ask Saambha")
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, REQUEST_SPEECH)
        } catch (e: ActivityNotFoundException) {
            reply.text = "Speech input isn't available on this watch."
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_SPEECH) return
        val text = data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)?.firstOrNull()
        if (resultCode != RESULT_OK || text.isNullOrBlank()) {
            if (reply.text.isEmpty()) reply.text = "Tap Ask to try again."
            return
        }
        heard.text = "“$text”"
        reply.text = "Thinking…"
        setBusy(true)
        thread {
            val result = runCatching { saambha.ask(text, onStatus = { msg -> runOnUiThread { status.text = msg } }) }
            runOnUiThread {
                setBusy(false)
                result.onSuccess { status.text = "Connected to ${saambha.serverName}" }
                reply.text = result.getOrElse { it.message ?: "Couldn't reach Saambha" }
                buzz()
            }
        }
    }

    private fun setBusy(value: Boolean) {
        busy = value
        askButton.isEnabled = !value
        if (value) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    private fun buzz() {
        getSystemService(Vibrator::class.java)
            ?.vibrate(VibrationEffect.createOneShot(120, VibrationEffect.DEFAULT_AMPLITUDE))
    }

    private fun buildLayout(): View {
        val pad = dp(24)

        // Connected: status, conversation, Ask.
        status = text(11f, Color.GRAY)
        heard = text(13f, Color.GRAY).apply { setPadding(0, dp(6), 0, 0) }
        reply = text(15f, Color.WHITE).apply { setPadding(0, dp(8), 0, dp(8)) }
        askButton = Button(this).apply {
            text = "Ask"
            setOnClickListener { if (!busy) startListening() }
        }
        val content = ScrollView(this).apply {
            isFillViewport = true
            visibility = View.GONE
            addView(LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER_HORIZONTAL
                setPadding(pad, dp(28), pad, dp(40))
                addView(status)
                addView(heard)
                addView(reply)
                addView(askButton)
                addView(Button(context).apply {
                    text = "Mouse"
                    setOnClickListener { startActivity(Intent(context, MouseActivity::class.java)) }
                })
            })
        }

        connectScreen = ConnectScreen(
            this, saambha, content,
            spinner = ProgressBar(this),
            message = text(13f, Color.LTGRAY).apply { setPadding(0, dp(10), 0, 0) },
            sidePaddingPx = pad,
            onConnected = { name ->
                status.text = "Connected to $name"
                if (listenWhenConnected) startListening()
                listenWhenConnected = false
            },
            onFailed = {
                listenWhenConnected = false
                buzz()
            },
        )

        return FrameLayout(this).apply {
            setBackgroundColor(Color.BLACK)
            addView(content, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
            addView(connectScreen.view, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        }
    }

    private fun text(sizeSp: Float, color: Int) = TextView(this).apply {
        setTextColor(color)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
        gravity = Gravity.CENTER
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

    companion object {
        private const val REQUEST_SPEECH = 1
    }
}
