package com.saambha.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import javax.net.ssl.SSLHandshakeException

class ServerFinderTest {

    private val settings = Settings(port = 8765)
    private val openPorts = mutableSetOf<String>()
    private val http = FakeHttp { response(404) }
    private val finder = ServerFinder(http, settings, portOpen = { _, host, port, _ -> port == 8765 && host in openPorts })

    private fun serverAt(host: String, name: String = "desktop") {
        openPorts += host
        val previous = http.handler
        http.handler = { if (it.url == "https://$host:8765/discover") response(200, "service" to "saambha", "name" to name) else previous(it) }
    }

    @Test
    fun `probe finds our server`() {
        serverAt("192.168.0.14", name = "anuj-popos")

        assertEquals(Server("https://192.168.0.14:8765", "anuj-popos"), finder.probe(null, "192.168.0.14"))
    }

    @Test
    fun `probe skips the HTTPS check when the port is closed`() {
        assertNull(finder.probe(null, "192.168.0.14"))
        assertTrue(http.requests.isEmpty())
    }

    @Test
    fun `something else on the port is not our server`() {
        openPorts += "192.168.0.20"
        http.handler = { response(200, "service" to "printer") }

        assertNull(finder.probe(null, "192.168.0.20"))
    }

    @Test
    fun `a server our CA didn't sign is not our server`() {
        openPorts += "192.168.0.20"
        http.handler = { throw SSLHandshakeException("untrusted") }

        assertNull(finder.probe(null, "192.168.0.20"))
    }

    @Test
    fun `nameless server is named by its address`() {
        openPorts += "192.168.0.14"
        http.handler = { response(200, "service" to "saambha") }

        assertEquals("192.168.0.14", finder.probe(null, "192.168.0.14")?.name)
    }

    @Test
    fun `find scans the hosts for the server`() {
        openPorts += "192.168.0.20"  // something else
        serverAt("192.168.0.14")

        val hosts = (1..254).map { "192.168.0.$it" }

        assertEquals("https://192.168.0.14:8765", finder.find(null, hosts)?.url)
    }

    @Test
    fun `find returns null when nothing answers`() {
        assertNull(finder.find(null, (1..254).map { "192.168.0.$it" }))
        assertNull(finder.find(null, emptyList()))
    }
}
