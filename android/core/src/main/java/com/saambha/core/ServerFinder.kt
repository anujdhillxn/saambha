package com.saambha.core

import android.net.Network
import java.net.InetSocketAddress
import java.util.concurrent.ExecutorCompletionService
import java.util.concurrent.Executors
import javax.net.SocketFactory

data class Server(val url: String, val name: String)

interface ServerLocator {
    /** The server at [host], or null if it isn't there (or isn't ours). */
    fun probe(network: Network?, host: String): Server?

    /** The first of [hosts] that is the server, or null. */
    fun find(network: Network?, hosts: List<String>): Server?
}

/**
 * Finds the saambha server: a host with the port open that answers GET /discover over HTTPS.
 * [http] only trusts our CA, so another device answering on the port is ignored.
 */
class ServerFinder(
    private val http: HttpClient,
    private val settings: Settings,
    private val portOpen: (network: Network?, host: String, port: Int, timeoutMs: Int) -> Boolean = ::tcpPortOpen,
) : ServerLocator {

    override fun probe(network: Network?, host: String): Server? {
        if (!portOpen(network, host, settings.port, settings.portTimeoutMs)) return null
        return try {
            val url = "https://$host:${settings.port}"
            val res = http.request(network, "GET", "$url/discover",
                connectTimeoutMs = settings.discoverConnectTimeoutMs, readTimeoutMs = settings.discoverReadTimeoutMs)
            if (res.ok && res.json.optString("service") == "saambha") {
                Server(url, res.json.optString("name").ifEmpty { host })
            } else null
        } catch (e: Exception) {
            null  // includes TLS failures: not our server
        }
    }

    override fun find(network: Network?, hosts: List<String>): Server? {
        if (hosts.isEmpty()) return null
        val pool = Executors.newFixedThreadPool(settings.scanThreads)
        try {
            val results = ExecutorCompletionService<Server?>(pool)
            hosts.forEach { host -> results.submit { probe(network, host) } }
            repeat(hosts.size) {
                results.take().get()?.let { return it }
            }
            return null
        } finally {
            pool.shutdownNow()
        }
    }
}

fun tcpPortOpen(network: Network?, host: String, port: Int, timeoutMs: Int): Boolean {
    val socketFactory = network?.socketFactory ?: SocketFactory.getDefault()
    return try {
        socketFactory.createSocket().use { it.connect(InetSocketAddress(host, port), timeoutMs) }
        true
    } catch (e: Exception) {
        false
    }
}
