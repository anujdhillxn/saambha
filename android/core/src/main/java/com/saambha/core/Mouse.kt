package com.saambha.core

import android.net.Network
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONObject
import java.net.InetAddress
import java.net.Socket
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import javax.net.SocketFactory

enum class MouseButton(val wire: String) { LEFT("left"), RIGHT("right") }

/**
 * The desktop's mouse. [timeMs] is when it happened on this device (SystemClock.uptimeMillis, as in
 * MotionEvent.eventTime): the server plays them at that pace, whatever bunches the network makes.
 */
interface Mouse {
    fun move(dx: Int, dy: Int, timeMs: Long)
    fun click(button: MouseButton, timeMs: Long)

    /** Positive scrolls up, like turning a wheel away from you. */
    fun scroll(notches: Int, timeMs: Long)
}

/** The desktop's mouse while connected to it; closing disconnects. */
interface MouseConnection : Mouse, AutoCloseable

/** Moves the mouse through the server's /mouse WebSocket; holds the network [lease] until closed. */
internal class RemoteMouse(private val socket: WebSocketConnection, private val lease: AutoCloseable) : MouseConnection {

    override fun move(dx: Int, dy: Int, timeMs: Long) =
        send(JSONObject().put("type", "move").put("dx", dx).put("dy", dy), timeMs)

    override fun click(button: MouseButton, timeMs: Long) =
        send(JSONObject().put("type", "click").put("button", button.wire), timeMs)

    override fun scroll(notches: Int, timeMs: Long) =
        send(JSONObject().put("type", "scroll").put("notches", notches), timeMs)

    override fun close() {
        socket.close()
        lease.close()
    }

    private fun send(message: JSONObject, timeMs: Long) = socket.send(message.put("t", timeMs).toString())
}

interface WebSocketClient {
    /**
     * Opens a WebSocket to [url] over [network] (or the default one). Messages sent before it's
     * open wait for it. [onClosed] gets why, if it closes or fails to open other than by [WebSocketConnection.close].
     */
    fun open(network: Network?, url: String, token: String?, onClosed: (reason: String) -> Unit): WebSocketConnection
}

interface WebSocketConnection : AutoCloseable {
    fun send(text: String)
}

class OkHttpWebSockets(tls: PinnedTls, settings: Settings) : WebSocketClient {

    private val client = OkHttpClient.Builder()
        .sslSocketFactory(tls.socketFactory, tls.trustManager)
        .hostnameVerifier(tls.anyHostname)
        .connectTimeout(settings.connectTimeoutMs.toLong(), TimeUnit.MILLISECONDS)
        .pingInterval(settings.mousePingSeconds, TimeUnit.SECONDS)  // so a dropped connection is noticed
        .build()

    override fun open(network: Network?, url: String, token: String?, onClosed: (String) -> Unit): WebSocketConnection {
        val sockets = network?.socketFactory ?: SocketFactory.getDefault()
        val http = client.newBuilder().socketFactory(NoDelaySocketFactory(sockets)).build()
        val request = Request.Builder().url(url).apply { token?.let { header("Authorization", "Bearer $it") } }.build()
        val closedByUs = AtomicBoolean(false)
        val socket = http.newWebSocket(request, object : WebSocketListener() {
            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                webSocket.close(1000, null)
                if (!closedByUs.get()) onClosed(reason.ifEmpty { "Disconnected" })
            }

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                if (!closedByUs.get()) onClosed(response?.let { "Server error ${it.code}" } ?: t.message ?: "Disconnected")
            }
        })
        return object : WebSocketConnection {
            override fun send(text: String) {
                socket.send(text)
            }

            override fun close() {
                closedByUs.set(true)
                socket.close(1000, null)
            }
        }
    }
}

/**
 * Sends each message at once: by default TCP holds small ones back to send them together, and the
 * desktop, seeing moves arrive in bunches, speeds the pointer up and down in jerks.
 */
private class NoDelaySocketFactory(private val sockets: SocketFactory) : SocketFactory() {
    override fun createSocket(): Socket = sockets.createSocket().noDelay()
    override fun createSocket(host: String, port: Int): Socket = sockets.createSocket(host, port).noDelay()
    override fun createSocket(host: String, port: Int, localHost: InetAddress, localPort: Int): Socket =
        sockets.createSocket(host, port, localHost, localPort).noDelay()
    override fun createSocket(host: InetAddress, port: Int): Socket = sockets.createSocket(host, port).noDelay()
    override fun createSocket(address: InetAddress, port: Int, localAddress: InetAddress, localPort: Int): Socket =
        sockets.createSocket(address, port, localAddress, localPort).noDelay()

    private fun Socket.noDelay() = apply { tcpNoDelay = true }
}
