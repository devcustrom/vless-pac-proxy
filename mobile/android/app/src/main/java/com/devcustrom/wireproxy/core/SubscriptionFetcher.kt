package com.devcustrom.wireproxy.core

import java.util.Base64
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.TimeUnit

object SubscriptionFetcher {
    private val client = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    suspend fun fetch(url: String): String = withContext(Dispatchers.IO) {
        require(url.startsWith("https://") || url.startsWith("http://")) {
            "Подписка должна быть http(s)-ссылкой"
        }
        val request = Request.Builder().url(url).get().build()
        client.newCall(request).execute().use { response ->
            check(response.isSuccessful) { "HTTP ${response.code}" }
            response.body?.string().orEmpty()
        }
    }

    fun links(text: String): List<String> {
        var body = text
        if (!body.contains("://")) {
            val compact = body.filterNot(Char::isWhitespace)
            val decoded = runCatching {
                String(Base64.getDecoder().decode(compact), Charsets.UTF_8)
            }.getOrNull()
            if (decoded != null) body = decoded
        }
        return body.lineSequence()
            .map(String::trim)
            .filter { it.isNotEmpty() && !it.startsWith("#") }
            .filter {
                val lower = it.lowercase()
                lower.startsWith("vless://") ||
                    lower.startsWith("vmess://") ||
                    lower.startsWith("trojan://") ||
                    lower.startsWith("ss://")
            }
            .toList()
    }
}
