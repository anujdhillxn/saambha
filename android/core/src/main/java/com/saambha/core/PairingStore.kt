package com.saambha.core

import android.content.SharedPreferences

/** The server this device is paired with, and its token. */
interface PairingStore {
    val server: Server?
    val token: String?
    fun save(server: Server, token: String)
    fun clearToken()
}

class PrefsPairingStore(private val prefs: SharedPreferences) : PairingStore {

    override val server: Server?
        get() {
            val url = prefs.getString(KEY_URL, null) ?: return null
            return Server(url, prefs.getString(KEY_NAME, null) ?: url)
        }

    override val token: String? get() = prefs.getString(KEY_TOKEN, null)

    override fun save(server: Server, token: String) {
        prefs.edit()
            .putString(KEY_URL, server.url)
            .putString(KEY_NAME, server.name)
            .putString(KEY_TOKEN, token)
            .apply()
    }

    override fun clearToken() {
        prefs.edit().remove(KEY_TOKEN).apply()
    }

    private companion object {
        const val KEY_URL = "server_url"
        const val KEY_NAME = "server_name"
        const val KEY_TOKEN = "token"
    }
}
