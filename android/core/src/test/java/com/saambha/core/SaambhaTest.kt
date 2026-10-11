package com.saambha.core

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.IOException

class SaambhaTest {

    private val desktop = Server("https://192.168.0.14:8765", "anuj-popos")
    private val validTokens = mutableSetOf("old-token")

    /** The server's side: /status checks the token, /pair issues "new-token", /command echoes. */
    private val http = FakeHttp { req ->
        val path = req.url.substringAfter("8765")
        when {
            path == "/pair" -> response(200, "token" to "new-token", "name" to "anuj-popos").also { validTokens += "new-token" }
            req.token !in validTokens -> response(401, "detail" to "Not paired")
            path == "/status" -> response(200, "ok" to true)
            path == "/command" -> response(200, "text" to "You said: ${req.body!!.getString("text")}")
            else -> response(404)
        }
    }
    private val locator = FakeLocator(mapOf("192.168.0.14" to desktop))
    private val lan = FakeLocalNetwork()
    private val store = MemoryPairingStore()
    private val sockets = FakeWebSockets()
    private val saambha = Saambha(http, locator, lan, store, sockets, deviceName = "Pixel Watch 4", settings = Settings())

    private fun pairedWith(server: Server, token: String = "old-token") {
        store.save(server, token)
    }

    // connect()

    @Test
    fun `paired with a reachable server - no scan, no pairing`() {
        pairedWith(desktop)

        assertEquals("anuj-popos", saambha.connect())

        assertNull(locator.scanned)
        assertEquals(listOf("GET https://192.168.0.14:8765/status"), http.paths())
        assertEquals("old-token", http.requests.single().token)
    }

    @Test
    fun `desktop changed address - rescans and keeps the token`() {
        pairedWith(Server("https://192.168.0.7:8765", "anuj-popos"))

        saambha.connect()

        assertEquals(listOf("192.168.0.11", "192.168.0.14"), locator.scanned)
        assertEquals(desktop, store.server)
        assertEquals("old-token", store.token)
        assertTrue(http.paths().none { it.endsWith("/pair") })
    }

    @Test
    fun `first launch - scans, asks the desktop and saves the token`() {
        val awaiting = mutableListOf<String>()

        assertEquals("anuj-popos", saambha.connect(onAwaitingApproval = { awaiting += it }))

        assertEquals(listOf("anuj-popos"), awaiting)
        val pair = http.requests.single { it.url.endsWith("/pair") }
        assertEquals("Pixel Watch 4", pair.body!!.getString("name"))
        assertEquals(desktop, store.server)
        assertEquals("new-token", store.token)
    }

    @Test
    fun `token revoked - pairs again`() {
        pairedWith(desktop, token = "revoked")

        saambha.connect()

        assertEquals("new-token", store.token)
    }

    @Test
    fun `denied on the desktop`() {
        http.handler = { response(403, "detail" to "Denied on the desktop") }

        val error = assertThrows(IOException::class.java) { saambha.connect() }

        assertEquals("Denied on the desktop", error.message)
        assertNull(store.token)
    }

    @Test
    fun `not on Wi-Fi`() {
        lan.hosts = null

        val error = assertThrows(IOException::class.java) { saambha.connect() }

        assertEquals("Connect to your home Wi-Fi first", error.message)
    }

    @Test
    fun `no server on this Wi-Fi`() {
        locator.servers = emptyMap()

        val error = assertThrows(IOException::class.java) { saambha.connect() }

        assertEquals("No Saambha server found on this Wi-Fi", error.message)
    }

    @Test
    fun `the network is released even when connecting fails`() {
        locator.servers = emptyMap()

        runCatching { saambha.connect() }
        runCatching { saambha.connect() }

        assertEquals(2, lan.acquired)
        assertEquals(2, lan.closed)
    }

    @Test
    fun `server name comes from the pairing`() {
        assertNull(saambha.serverName)
        pairedWith(desktop)

        assertEquals("anuj-popos", saambha.serverName)
    }

    // ask()

    @Test
    fun `ask sends the command with the token and returns the reply`() {
        pairedWith(desktop)

        assertEquals("You said: what time is it", saambha.ask("what time is it", speakOnDesktop = false))

        val command = http.requests.last()
        assertEquals("POST https://192.168.0.14:8765/command", "${command.method} ${command.url}")
        assertEquals("none", command.body!!.getString("speak"))
        assertEquals("old-token", command.token)
    }

    @Test
    fun `ask from the watch is spoken on the desktop`() {
        pairedWith(desktop)

        saambha.ask("hello")

        assertEquals("desktop", http.requests.last().body!!.getString("speak"))
    }

    @Test
    fun `token revoked between the check and the command - pairs again once and retries`() {
        pairedWith(desktop)
        var commands = 0
        val server = http.handler
        http.handler = { req ->
            if (req.url.endsWith("/command") && commands++ == 0) {
                validTokens.remove("old-token")
                response(401, "detail" to "Not paired")
            } else server(req)
        }

        assertEquals("You said: hi", saambha.ask("hi"))

        assertEquals("new-token", store.token)
        assertEquals("new-token", http.requests.last().token)
    }

    @Test
    fun `server error is reported with its detail`() {
        pairedWith(desktop)
        val server = http.handler
        http.handler = { req -> if (req.url.endsWith("/command")) response(400, "detail" to "Empty command") else server(req) }

        val error = assertThrows(IOException::class.java) { saambha.ask(" ") }

        assertEquals("Empty command", error.message)
    }

    @Test
    fun `ask shows progress while it has to search`() {
        val status = mutableListOf<String>()

        saambha.ask("hi", onStatus = { status += it })

        assertEquals(listOf("Searching the network…", "Approve on anuj-popos…"), status)
    }

    // openMouse()

    @Test
    fun `mouse - a WebSocket to the paired server that sends what the mouse does`() {
        pairedWith(desktop)

        saambha.openMouse(onClosed = {}).use { mouse ->
            mouse.move(12, -3, timeMs = 1000)
            mouse.click(MouseButton.RIGHT, timeMs = 1010)
            mouse.scroll(-2, timeMs = 1020)
        }

        val socket = sockets.opened.single()
        assertEquals("wss://192.168.0.14:8765/mouse", socket.url)
        assertEquals("old-token", socket.token)
        assertEquals(
            listOf(
                mapOf("type" to "move", "dx" to 12, "dy" to -3, "t" to 1000),
                mapOf("type" to "click", "button" to "right", "t" to 1010),
                mapOf("type" to "scroll", "notches" to -2, "t" to 1020),
            ),
            socket.sent.map { JSONObject(it).toMap() },
        )
    }

    @Test
    fun `mouse - pairs first if it has to`() {
        saambha.openMouse(onClosed = {})

        assertEquals("new-token", sockets.opened.single().token)
    }

    @Test
    fun `mouse - keeps the network until it's closed`() {
        pairedWith(desktop)

        val mouse = saambha.openMouse(onClosed = {})
        assertEquals(0, lan.closed)
        mouse.close()

        assertEquals(1, lan.closed)
        assertTrue(sockets.opened.single().closed)
    }

    @Test
    fun `mouse - no server lets go of the network`() {
        locator.servers = emptyMap()

        assertThrows(IOException::class.java) { saambha.openMouse(onClosed = {}) }

        assertEquals(lan.acquired, lan.closed)
        assertTrue(sockets.opened.isEmpty())
    }

    @Test
    fun `mouse - hears why the server ended it`() {
        pairedWith(desktop)
        val reasons = mutableListOf<String>()

        saambha.openMouse(onClosed = { reasons += it })
        sockets.opened.single().onClosed("Can't control the mouse: Permission denied")

        assertEquals(listOf("Can't control the mouse: Permission denied"), reasons)
    }
}
