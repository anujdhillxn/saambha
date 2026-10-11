package com.saambha.core

import android.net.Network
import org.json.JSONObject

/** One request as the fake server saw it. */
data class Request(val method: String, val url: String, val body: JSONObject?, val token: String?)

/** An HttpClient answered by [handler], recording every request. */
class FakeHttp(var handler: (Request) -> HttpResponse) : HttpClient {
    val requests = mutableListOf<Request>()

    override fun request(
        network: Network?, method: String, url: String, body: JSONObject?, token: String?,
        connectTimeoutMs: Int, readTimeoutMs: Int,
    ): HttpResponse {
        val request = Request(method, url, body, token)
        requests += request
        return handler(request)
    }

    fun paths() = requests.map { "${it.method} ${it.url}" }
}

fun response(code: Int, vararg fields: Pair<String, Any>) =
    HttpResponse(code, JSONObject().apply { fields.forEach { (k, v) -> put(k, v) } })

/** A ServerLocator where [servers] maps each host that has a server to it. */
class FakeLocator(var servers: Map<String, Server> = emptyMap()) : ServerLocator {
    var scanned: List<String>? = null

    override fun probe(network: Network?, host: String) = servers[host]

    override fun find(network: Network?, hosts: List<String>): Server? {
        scanned = hosts
        return hosts.firstNotNullOfOrNull { servers[it] }
    }
}

/** A LAN of [hosts]; null means not on a LAN at all. */
class FakeLocalNetwork(var hosts: List<String>? = listOf("192.168.0.11", "192.168.0.14")) : LocalNetwork {
    var acquired = 0
    var closed = 0

    override fun acquire(): LocalNetwork.Lease {
        acquired++
        return object : LocalNetwork.Lease {
            override val network: Network? = null
            override fun subnetHosts() = hosts ?: throw NotOnLanException()
            override fun close() {
                closed++
            }
        }
    }
}

class MemoryPairingStore(override var server: Server? = null, override var token: String? = null) : PairingStore {
    override fun save(server: Server, token: String) {
        this.server = server
        this.token = token
    }

    override fun clearToken() {
        token = null
    }
}

/** WebSockets that record what's sent; [close] stands in for the server or network ending one. */
class FakeWebSockets : WebSocketClient {
    inner class Socket(val url: String, val token: String?, val onClosed: (String) -> Unit) : WebSocketConnection {
        val sent = mutableListOf<String>()
        var closed = false

        override fun send(text: String) {
            sent += text
        }

        override fun close() {
            closed = true
        }
    }

    val opened = mutableListOf<Socket>()

    override fun open(network: Network?, url: String, token: String?, onClosed: (String) -> Unit) =
        Socket(url, token, onClosed).also { opened += it }
}

/** A Mouse that records what it's told, and when it happened. */
class RecordingMouse : Mouse {
    val done = mutableListOf<String>()
    val times = mutableListOf<Long>()

    override fun move(dx: Int, dy: Int, timeMs: Long) {
        done += "move $dx $dy"
        times += timeMs
    }

    override fun click(button: MouseButton, timeMs: Long) {
        done += "click ${button.wire}"
        times += timeMs
    }

    override fun scroll(notches: Int, timeMs: Long) {
        done += "scroll $notches"
        times += timeMs
    }
}
