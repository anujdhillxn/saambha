package com.saambha.core

import android.content.Context
import android.net.ConnectivityManager
import android.os.Build
import org.json.JSONObject
import java.io.IOException
import java.net.URI

/** What the apps need from the server. All calls block — run them off the main thread. */
interface SaambhaClient {
    val serverName: String?

    /**
     * Makes sure there's a reachable server this device is paired with: checks the saved one
     * first, otherwise scans the network and pairs (Allow/Deny in the desktop's tray icon).
     * [onAwaitingApproval] is called with the server's name when the user has to click Allow
     * on the desktop. Returns the server's name.
     */
    fun connect(onAwaitingApproval: (String) -> Unit = {}): String

    /**
     * Sends [text] as a command and returns Claude's reply. Connects first if needed (rescanning
     * if the desktop's address changed, pairing if this device isn't paired yet).
     */
    fun ask(text: String, speakOnDesktop: Boolean = true, onStatus: (String) -> Unit = {}): String

    /**
     * Connects (as [connect] does) and takes over the desktop's mouse until it's closed.
     * [onClosed] gets why, if the server or the network ends it first.
     */
    fun openMouse(onClosed: (reason: String) -> Unit): MouseConnection
}

class Saambha(
    private val http: HttpClient,
    private val locator: ServerLocator,
    private val localNetwork: LocalNetwork,
    private val store: PairingStore,
    private val webSockets: WebSocketClient,
    private val deviceName: String,
    private val settings: Settings,
) : SaambhaClient {

    override val serverName: String? get() = store.server?.name

    override fun connect(onAwaitingApproval: (String) -> Unit): String =
        localNetwork.acquire().use { lease -> connect(lease, onStatus = {}, onAwaitingApproval).name }

    override fun ask(text: String, speakOnDesktop: Boolean, onStatus: (String) -> Unit): String =
        localNetwork.acquire().use { lease ->
            var server = connect(lease, onStatus)
            var res = command(lease, server.url, text, speakOnDesktop)
            if (res.code == HttpResponse.UNAUTHORIZED) {
                // Revoked since the check: pair again, once.
                store.clearToken()
                server = connect(lease, onStatus)
                res = command(lease, server.url, text, speakOnDesktop)
            }
            if (!res.ok) throw IOException(res.detail)
            res.json.optString("text").ifEmpty { "(no reply)" }
        }

    override fun openMouse(onClosed: (String) -> Unit): MouseConnection {
        val lease = localNetwork.acquire()
        try {
            val server = connect(lease, onStatus = {})
            val url = server.url.replaceFirst("https://", "wss://") + "/mouse"
            return RemoteMouse(webSockets.open(lease.network, url, store.token, onClosed), lease)
        } catch (e: Exception) {
            lease.close()
            throw e
        }
    }

    private fun connect(
        lease: LocalNetwork.Lease,
        onStatus: (String) -> Unit,
        onAwaitingApproval: (String) -> Unit = {},
    ): Server {
        val token = store.token
        val saved = store.server
        if (saved != null && token != null) {
            val host = URI(saved.url).host
            if (host != null && locator.probe(lease.network, host) != null &&
                tokenWorks(lease, saved.url, token)
            ) return saved
        }

        onStatus("Searching the network…")
        val server = try {
            locator.find(lease.network, lease.subnetHosts())
        } catch (e: NotOnLanException) {
            throw IOException("Connect to your home Wi-Fi first")
        } ?: throw IOException("No Saambha server found on this Wi-Fi")

        if (token != null && tokenWorks(lease, server.url, token)) {
            store.save(server, token)
            return server
        }

        onStatus("Approve on ${server.name}…")
        onAwaitingApproval(server.name)
        val res = http.request(
            lease.network, "POST", "${server.url}/pair",
            body = JSONObject().put("name", deviceName),
            connectTimeoutMs = settings.connectTimeoutMs,
            readTimeoutMs = settings.pairReadTimeoutMs,
        )
        if (!res.ok) throw IOException(res.detail)
        store.save(server, res.json.getString("token"))
        return server
    }

    private fun tokenWorks(lease: LocalNetwork.Lease, url: String, token: String): Boolean =
        runCatching {
            http.request(lease.network, "GET", "$url/status", token = token,
                connectTimeoutMs = settings.connectTimeoutMs, readTimeoutMs = settings.readTimeoutMs).ok
        }.getOrDefault(false)

    private fun command(lease: LocalNetwork.Lease, url: String, text: String, speakOnDesktop: Boolean) =
        http.request(
            lease.network, "POST", "$url/command",
            body = JSONObject().put("text", text).put("speak", if (speakOnDesktop) "desktop" else "none"),
            token = store.token,
            connectTimeoutMs = settings.connectTimeoutMs,
            readTimeoutMs = settings.commandReadTimeoutMs,
        )

    companion object {
        /** The real thing: Wi-Fi, the bundled CA, and pairing saved in SharedPreferences. */
        fun create(context: Context, settings: Settings = Settings()): Saambha {
            val app = context.applicationContext
            val tls = PinnedTls.bundled(app)
            val http = PinnedHttpClient(tls)
            return Saambha(
                http = http,
                locator = ServerFinder(http, settings),
                localNetwork = WifiNetwork(app.getSystemService(ConnectivityManager::class.java), settings),
                store = PrefsPairingStore(app.getSharedPreferences("saambha", Context.MODE_PRIVATE)),
                webSockets = OkHttpWebSockets(tls, settings),
                deviceName = deviceName(),
                settings = settings,
            )
        }

        // Shown in the desktop's tray icon when it asks, e.g. "Google Pixel Watch 4".
        private fun deviceName(): String =
            if (Build.MODEL.startsWith(Build.MANUFACTURER, ignoreCase = true)) Build.MODEL
            else "${Build.MANUFACTURER} ${Build.MODEL}"
    }
}
