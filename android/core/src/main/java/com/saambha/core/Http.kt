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
import javax.net.ssl.SSLSocketFactory
import javax.net.ssl.TrustManagerFactory
import javax.net.ssl.X509TrustManager

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
 * TLS that trusts only [ca], the server's local CA.
 *
 * Hostname checks are skipped on purpose: the server is found by IP and its address can change,
 * and only the desktop holds the CA's private key, so "signed by our CA" is what identifies it.
 */
class PinnedTls(ca: Certificate) {
    val trustManager: X509TrustManager = run {
        val keyStore = KeyStore.getInstance(KeyStore.getDefaultType()).apply {
            load(null, null)
            setCertificateEntry("saambha", ca)
        }
        val tmf = TrustManagerFactory.getInstance(TrustManagerFactory.getDefaultAlgorithm()).apply { init(keyStore) }
        tmf.trustManagers.filterIsInstance<X509TrustManager>().single()
    }
    val socketFactory: SSLSocketFactory =
        SSLContext.getInstance("TLS").apply { init(null, arrayOf(trustManager), null) }.socketFactory
    val anyHostname = HostnameVerifier { _, _ -> true }

    companion object {
        /** Trusts the CA bundled as res/raw/saambha_ca (copied from server/ca.pem at build time). */
        fun bundled(context: Context) = PinnedTls(
            context.resources.openRawResource(R.raw.saambha_ca).use {
                CertificateFactory.getInstance("X.509").generateCertificate(it)
            }
        )
    }
}

/** HTTPS to the saambha server. */
class PinnedHttpClient(private val tls: PinnedTls) : HttpClient {

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
        conn.sslSocketFactory = tls.socketFactory
        conn.hostnameVerifier = tls.anyHostname
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
}
