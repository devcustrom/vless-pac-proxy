package com.devcustrom.wireproxy.telegram

import java.net.InetAddress
import java.util.concurrent.TimeUnit
import okhttp3.Dns
import okhttp3.OkHttpClient
import okhttp3.Request

/**
 * Cloudflare-fronted Telegram WS gateway domains. Telegram's own
 * `kwsN.web.telegram.org` hostnames are blocked by some ISPs; these domains
 * front the same `/apiws` endpoint through Cloudflare.
 *
 * The built-in list is the same set the desktop tgws sidecar ships with. It is
 * refreshed from the upstream list in the background, but a failed refresh
 * never prevents the proxy from starting.
 */
object TelegramRoutes {
    private const val LIST_URL =
        "https://raw.githubusercontent.com/Flowseal/tg-ws-proxy/main/.github/cfproxy-domains.txt"
    private const val GITHUB_IP = "185.199.109.133"

    private val encodedDefaults = listOf(
        "virkgj.com",
        "vmmzovy.com",
        "mkuosckvso.com",
        "zaewayzmplad.com",
        "twdmbzcm.com",
        "awzwsldi.com",
        "clngqrflngqin.com",
        "tjacxbqtj.com",
        "bxaxtxmrw.com",
        "dmohrsgmohcrwb.com",
        "vwbmtmoi.com",
        "khgrre.com",
        "ulihssf.com",
        "tmhqsdqmfpmk.com",
        "xwuwoqbm.com",
        "orgcnunpj.com",
        "zhkuldz.com",
        "zypoljnslxa.com",
        "efabnxaowuzs.com",
        "zaftuzsftqdq.com",
    )

    @Volatile
    var domains: List<String> = encodedDefaults.map(::decode)

    private val client = OkHttpClient.Builder()
        .connectTimeout(8, TimeUnit.SECONDS)
        .readTimeout(10, TimeUnit.SECONDS)
        .dns(object : Dns {
            override fun lookup(hostname: String): List<InetAddress> {
                if (hostname == "raw.githubusercontent.com") {
                    return listOf(InetAddress.getByName(GITHUB_IP))
                }
                return Dns.SYSTEM.lookup(hostname)
            }
        })
        .build()

    /** Fetches a fresh domain list; returns true when it was replaced. */
    fun refresh(): Boolean {
        val request = Request.Builder().url(LIST_URL).get().build()
        val fetched = runCatching {
            client.newCall(request).execute().use { response ->
                if (!response.isSuccessful) return false
                response.body?.string().orEmpty()
            }
        }.getOrNull().orEmpty()
        val decoded = fetched.lineSequence()
            .map(String::trim)
            .filter { it.isNotEmpty() && !it.startsWith("#") }
            .map(::decode)
            .filter(::isValidDomain)
            .distinct()
            .toList()
        if (decoded.size < 3) return false
        domains = decoded
        return true
    }

    private fun decode(value: String): String {
        if (!value.endsWith(".com")) return value
        val letters = value.dropLast(4)
        val shift = letters.count(Char::isLetter)
        val decoded = letters.map { char ->
            if (!char.isLetter()) {
                char
            } else {
                val base = if (char.isLowerCase()) 'a'.code else 'A'.code
                (((char.code - base - shift) % 26 + 26) % 26 + base).toChar()
            }
        }.joinToString("")
        return decoded + ".co.uk"
    }

    private fun isValidDomain(domain: String): Boolean {
        if (domain.length < 4 || domain.length > 253) return false
        if (domain.startsWith(".") || domain.endsWith(".")) return false
        val labels = domain.split('.')
        if (labels.size < 2) return false
        return labels.all { label ->
            label.isNotEmpty() && label.length <= 63 &&
                !label.startsWith("-") && !label.endsWith("-") &&
                label.all { it.isLetterOrDigit() || it == '-' }
        } && labels.last().length >= 2 && labels.last().any(Char::isLetter)
    }
}
