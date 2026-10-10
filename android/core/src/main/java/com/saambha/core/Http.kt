package com.saambha.core

import android.content.Context
import android.net.Network
import org.json.JSONObject
import java.net.URL
import java.security.KeyStore
import java.security.cert.Certificate
import java.security.cert.CertificateFactory
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.HttpsURLConnection
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManagerFactory

class HttpResponse(val code: Int, val json: JSONObject) {
    val ok get() = code in 200..299
    val detail: String get() = json.optString("detail").ifEmpty { "Server error $code" }

    companion object {
        const val UNAUTHORIZED = 401
    }
}

interface HttpClient {
    /** Sends [body] (if any) as JSON over [network] (or the default network) and parses the JSON reply. */
    fun request(
        network: Network?,
        method: String,
        url: String,
        body: JSONObject? = null,
        token: String? = null,
        connectTimeoutMs: Int,
        readTimeoutMs: Int,
    ): HttpResponse
}

/**
 * HTTPS to the saambha server, trusting only [ca] (the server's local CA).
 *
 * Hostname checks are skipped on purpose: the server is found by IP and its address can change,
 * and only the desktop holds the CA's private key, so "signed by our CA" is what identifies it.
 */
class PinnedHttpClient(ca: Certificate) : HttpClient {

    private val sslContext: SSLContext = run {
        val keyStore = KeyStore.getInstance(KeyStore.getDefaultType()).apply {
            load(null, null)
            setCertificateEntry("saambha", ca)
        }
        val tmf = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm()).apply { init(keyStore) }
        SSLContext.getInstance("TLS").apply { init(null, tmf.trustManagers, null) }
    }

    override fun request(
        network: Network?,
        method: String,
        url: String,
        body: JSONObject?,
        token: String?,
        connectTimeoutMs: Int,
        readTimeoutMs: Int,
    ): HttpResponse {
        val u = URL(url)
        val conn = (network?.openConnection(u) ?: u.openConnection()) as HttpsURLConnection
        conn.sslSocketFactory = sslContext.socketFactory
        conn.hostnameVerifier = HostnameVerifier { _, _ -> true }
        conn.requestMethod = method
        conn.connectTimeout = connectTimeoutMs
        conn.readTimeout = readTimeoutMs
        token?.let { conn.setRequestProperty("Authorization", "Bearer $it") }
        try {
            if (body != null) {
                conn.doOutput = true
                conn.setRequestProperty("Content-Type", "application/json")
                conn.outputStream.use { it.write(body.toString().toByteArray()) }
            }
            val code = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val text = stream?.bufferedReader()?.use { it.readText() }.orEmpty()
            val json = runCatching { JSONObject(text) }.getOrDefault(JSONObject())
            return HttpResponse(code, json)
        } finally {
            conn.disconnect()
        }
    }

    companion object {
        /** Trusts the CA bundled as res/raw/saambha_ca (copied from server/ca.pem at build time). */
        fun withBundledCa(context: Context) = PinnedHttpClient(
            context.resources.openRawResource(R.raw.saambha_ca).use {
                CertificateFactory.getInstance("X.509").generateCertificate(it)
            }
        )
    }
}
