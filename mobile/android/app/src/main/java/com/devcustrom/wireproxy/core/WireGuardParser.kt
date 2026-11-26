package com.devcustrom.wireproxy.core

import org.json.JSONArray
import org.json.JSONObject

/**
 * Converts a standard WireGuard client config into the Xray WireGuard outbound.
 * Mirrors mobile/rust/src/wireguard.rs so both paths accept the same files.
 */
object WireGuardParser {
    fun isWireGuard(text: String): Boolean =
        text.contains("[Interface]", ignoreCase = true) &&
            text.contains("[Peer]", ignoreCase = true)

    fun buildOutbound(raw: String, tag: String = "wireguard-out"): JSONObject {
        val config = parse(raw)
        val peers = JSONArray()
        for (peer in config.peers) {
            val item = JSONObject()
                .put("publicKey", peer.publicKey)
                .put("endpoint", peer.endpoint)
                .put("allowedIPs", JSONArray(peer.allowedIps))
            peer.presharedKey?.let { item.put("preSharedKey", it) }
            if (peer.keepAlive > 0) item.put("keepAlive", peer.keepAlive)
            peers.put(item)
        }
        val settings = JSONObject()
            .put("secretKey", config.privateKey)
            .put("address", JSONArray(config.addresses))
            .put("peers", peers)
            .put("mtu", config.mtu)
            .put("noKernelTun", true)
        if (config.dns.isNotEmpty()) {
            settings.put("remoteDNS", JSONArray(config.dns))
        }
        return JSONObject()
            .put("tag", tag)
            .put("protocol", "wireguard")
            .put("settings", settings)
    }

    private data class Peer(
        var publicKey: String,
        var presharedKey: String? = null,
        var endpoint: String = "",
        val allowedIps: MutableList<String> = mutableListOf(),
        var keepAlive: Int = 0,
    )

    private data class Config(
        var privateKey: String = "",
        val addresses: MutableList<String> = mutableListOf(),
        val dns: MutableList<String> = mutableListOf(),
        var mtu: Int = 1420,
        val peers: MutableList<Peer> = mutableListOf(),
    )

    private fun parse(raw: String): Config {
        val config = Config()
        var section = ""
        var peer: Peer? = null

        fun flushPeer() {
            peer?.let {
                require(it.publicKey.isNotBlank()) { "WireGuard: нет PublicKey" }
                require(it.endpoint.isNotBlank()) { "WireGuard: нет Endpoint" }
                if (it.allowedIps.isEmpty()) {
                    it.allowedIps += listOf("0.0.0.0/0", "::/0")
                }
                config.peers += it
            }
            peer = null
        }

        raw.lineSequence().forEach { rawLine ->
            val line = rawLine.substringBefore('#').trim()
            if (line.isEmpty()) return@forEach
            if (line.startsWith("[") && line.endsWith("]")) {
                flushPeer()
                section = line.substring(1, line.length - 1).trim()
                if (section.equals("Peer", ignoreCase = true)) peer = Peer("")
                return@forEach
            }
            val key = line.substringBefore('=').trim()
            val value = line.substringAfter('=', "").trim()
            when {
                section.equals("Interface", ignoreCase = true) -> when (key.lowercase()) {
                    "privatekey" -> config.privateKey = value
                    "address" -> config.addresses += split(value).map(::normalizeAddress)
                    "dns" -> config.dns += split(value)
                    "mtu" -> config.mtu = value.toIntOrNull() ?: 1420
                }
                section.equals("Peer", ignoreCase = true) -> {
                    val current = peer ?: return@forEach
                    when (key.lowercase()) {
                        "publickey" -> current.publicKey = value
                        "presharedkey" -> current.presharedKey = value
                        "endpoint" -> current.endpoint = value
                        "allowedips" -> current.allowedIps += split(value)
                        "persistentkeepalive" -> current.keepAlive = value.toIntOrNull() ?: 0
                    }
                }
            }
        }
        flushPeer()
        require(config.privateKey.isNotBlank()) { "WireGuard: нет PrivateKey" }
        require(config.addresses.isNotEmpty()) { "WireGuard: нет Address" }
        require(config.peers.isNotEmpty()) { "WireGuard: нет Peer" }
        return config
    }

    private fun split(value: String): List<String> =
        value.split(',').map(String::trim).filter(String::isNotEmpty)

    private fun normalizeAddress(value: String): String = when {
        value.contains('/') -> value
        value.contains(':') -> "$value/128"
        else -> "$value/32"
    }
}
