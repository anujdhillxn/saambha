package com.saambha.core

import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import java.net.Inet4Address
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class NotOnLanException : Exception("Not connected to Wi-Fi")

/** Access to the local network for the duration of a request. */
interface LocalNetwork {
    fun acquire(): Lease

    interface Lease : AutoCloseable {
        /** The network to use, or null for the default one. */
        val network: Network?

        /** The other addresses on this device's LAN. Throws [NotOnLanException] if it isn't on one. */
        fun subnetHosts(): List<String>
    }
}

/**
 * Asks the system for Wi-Fi for each request. On Wear OS this switches Wi-Fi on even when the
 * watch normally routes through the phone over Bluetooth (which can't reach the LAN). If no
 * Wi-Fi comes up in time, requests use the default network.
 */
class WifiNetwork(private val cm: ConnectivityManager, private val settings: Settings) : LocalNetwork {

    override fun acquire(): LocalNetwork.Lease {
        val latch = CountDownLatch(1)
        var wifi: Network? = null
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(n: Network) {
                wifi = n
                latch.countDown()
            }
        }
        val request = NetworkRequest.Builder().addTransportType(NetworkCapabilities.TRANSPORT_WIFI).build()
        try {
            cm.requestNetwork(request, callback)
            latch.await(settings.wifiTimeoutSeconds, TimeUnit.SECONDS)
        } catch (e: Exception) {
            // fall back to the default network
        }
        return object : LocalNetwork.Lease {
            override val network = wifi

            override fun subnetHosts(): List<String> {
                val net = network ?: cm.activeNetwork ?: throw NotOnLanException()
                val link = cm.getLinkProperties(net)?.linkAddresses
                    ?.firstOrNull { it.address is Inet4Address && it.address.isSiteLocalAddress }
                    ?: throw NotOnLanException()
                return subnetHosts(link.address.address, link.prefixLength)
            }

            override fun close() {
                runCatching { cm.unregisterNetworkCallback(callback) }
            }
        }
    }
}

/** The other addresses on [address]'s subnet, narrowed to its /24 if the subnet is bigger. */
fun subnetHosts(address: ByteArray, prefixLength: Int): List<String> {
    val ip = address.fold(0L) { acc, b -> (acc shl 8) or (b.toLong() and 0xFF) }
    val prefix = maxOf(prefixLength, 24)
    val mask = (0xFFFFFFFFL shl (32 - prefix)) and 0xFFFFFFFFL
    val base = ip and mask
    val size = 1L shl (32 - prefix)
    return (1 until size - 1)
        .map { base + it }
        .filter { it != ip }
        .map { n -> "${(n shr 24) and 0xFF}.${(n shr 16) and 0xFF}.${(n shr 8) and 0xFF}.${n and 0xFF}" }
}
