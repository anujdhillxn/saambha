package com.saambha.mobile

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.speech.RecognizerIntent
import android.speech.tts.TextToSpeech
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import com.saambha.core.ConnectScreen
import com.saambha.core.Saambha
import com.saambha.core.SaambhaClient
import kotlin.concurrent.thread

/**
 * Shows only a spinner while connecting to the saambha server (scan + pair if needed); pull down
 * to retry if that fails. Then: tap the mic, speak (the phone's own speech recognition), and the
 * words go to saambha. The reply is shown and spoken on the phone.
 */
class MainActivity : Activity() {

    private lateinit var saambha: SaambhaClient
    private lateinit var tts: TextToSpeech
    private var ttsReady = false

    private lateinit var connectScreen: ConnectScreen
    private lateinit var status: TextView
    private lateinit var heard: TextView
    private lateinit var reply: TextView
    private lateinit var micButton: ImageButton

    private var busy = false
    private var listenWhenConnected = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        saambha = Saambha.create(this)
        tts = TextToSpeech(this) { ttsReady = it == TextToSpeech.SUCCESS }
        setContentView(buildLayout())
        if (savedInstanceState == null) handleLaunch(intent)
        connectScreen.connect()
    }

    override fun onDestroy() {
        tts.shutdown()
        super.onDestroy()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleLaunch(intent)
    }

    /**
     * Opened from the launcher or by voice ("Hey Google, open Saambha", or a routine doing that), or by the
     * Quick Settings tile (with EXTRA_LISTEN): straight to speech input, as on the watch.
     */
    private fun handleLaunch(intent: Intent?) {
        val launched = intent?.action == Intent.ACTION_MAIN && intent.hasCategory(Intent.CATEGORY_LAUNCHER)
        if (!(launched || intent?.getBooleanExtra(EXTRA_LISTEN, false) == true) || busy) return
        if (connectScreen.connected) startListening() else listenWhenConnected = true
    }

    private fun startListening() {
        tts.stop()
        val intent = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH)
            .putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            .putExtra(RecognizerIntent.EXTRA_PROMPT, "Ask Saambha")
        try {
            @Suppress("DEPRECATION")
            startActivityForResult(intent, REQUEST_SPEECH)
        } catch (e: ActivityNotFoundException) {
            reply.text = "Speech recognition isn't available. Install or enable the Google app."
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_SPEECH) return
        val text = data?.getStringArrayListExtra(RecognizerIntent.EXTRA_RESULTS)?.firstOrNull()
        if (resultCode != RESULT_OK || text.isNullOrBlank()) return

        heard.text = "“$text”"
        reply.text = "Thinking…"
        setBusy(true)
        thread {
            val result = runCatching { saambha.ask(text, speakOnDesktop = false, onStatus = { msg -> runOnUiThread { status.text = msg } }) }
            runOnUiThread {
                setBusy(false)
                result.onSuccess { answer ->
                    status.text = "Connected to ${saambha.serverName}"
                    reply.text = answer
                    if (ttsReady) tts.speak(answer, TextToSpeech.QUEUE_FLUSH, null, "reply")
                }
                result.onFailure { reply.text = it.message ?: "Couldn't reach Saambha" }
            }
        }
    }

    private fun setBusy(value: Boolean) {
        busy = value
        micButton.isEnabled = !value
        micButton.alpha = if (value) 0.5f else 1f
        if (value) window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        else window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
    }

    private fun buildLayout(): View {
        // Connected: status, conversation, mic.
        status = text(13f, Color.GRAY)
        heard = text(16f, Color.GRAY).apply { setPadding(0, dp(32), 0, 0) }
        reply = text(19f, Color.WHITE).apply { setPadding(0, dp(12), 0, dp(32)) }
        micButton = ImageButton(this).apply {
            setImageResource(com.saambha.core.R.drawable.ic_mic)
            scaleType = android.widget.ImageView.ScaleType.CENTER_INSIDE
            background = GradientDrawable().apply {
                shape = GradientDrawable.OVAL
                setColor(0xFFC0392B.toInt())
            }
            contentDescription = "Speak"
            layoutParams = LinearLayout.LayoutParams(dp(132), dp(132))
            setOnClickListener { if (!busy) startListening() }
        }
        val replyArea = ScrollView(this).apply {
            layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f)
            addView(LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                addView(heard)
                addView(reply)
            })
        }
        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            setPadding(dp(24), dp(48), dp(24), dp(56))
            visibility = View.GONE
            addView(status)
            addView(replyArea)
            addView(micButton)
        }

        connectScreen = ConnectScreen(
            this, saambha, content,
            spinner = ProgressBar(this, null, android.R.attr.progressBarStyleLarge),
            message = text(16f, Color.LTGRAY).apply { setPadding(0, dp(24), 0, 0) },
            sidePaddingPx = dp(32),
            onConnected = { name ->
                status.text = "Connected to $name"
                if (listenWhenConnected) startListening()
                listenWhenConnected = false
            },
            onFailed = { listenWhenConnected = false },
        )

        return FrameLayout(this).apply {
            setBackgroundColor(Color.parseColor("#111111"))
            addView(content, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
            addView(connectScreen.view, FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT))
        }
    }

    private fun text(sizeSp: Float, color: Int) = TextView(this).apply {
        setTextColor(color)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, sizeSp)
        gravity = Gravity.CENTER
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT)
    }

    private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

    companion object {
        const val EXTRA_LISTEN = "listen"
        private const val REQUEST_SPEECH = 1
    }
}
