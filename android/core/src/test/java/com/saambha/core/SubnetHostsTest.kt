package com.saambha.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SubnetHostsTest {

    private fun hosts(ip: String, prefix: Int) =
        subnetHosts(ip.split(".").map { it.toInt().toByte() }.toByteArray(), prefix)

    @Test
    fun `home network scans every other address on the 24`() {
        val hosts = hosts("192.168.0.12", 24)

        assertEquals(253, hosts.size)
        assertEquals("192.168.0.1", hosts.first())
        assertEquals("192.168.0.254", hosts.last())
        assertTrue("192.168.0.14" in hosts)
    }

    @Test
    fun `skips own address, network address and broadcast`() {
        val hosts = hosts("192.168.0.12", 24)

        assertFalse("192.168.0.12" in hosts)
        assertFalse("192.168.0.0" in hosts)
        assertFalse("192.168.0.255" in hosts)
    }

    @Test
    fun `bigger subnet is narrowed to own 24`() {
        val hosts = hosts("10.1.7.200", 16)

        assertEquals(253, hosts.size)
        assertTrue(hosts.all { it.startsWith("10.1.7.") })
    }

    @Test
    fun `smaller subnet scans only itself`() {
        assertEquals(listOf("192.168.5.1"), hosts("192.168.5.2", 30))  // .0 network, .3 broadcast
    }

    @Test
    fun `bytes above 127 are read as unsigned`() {
        assertTrue("172.16.200.254" in hosts("172.16.200.130", 24))
    }
}
