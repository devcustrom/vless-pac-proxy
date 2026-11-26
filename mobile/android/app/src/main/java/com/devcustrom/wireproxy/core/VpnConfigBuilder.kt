package com.devcustrom.wireproxy.core

import org.json.JSONArray
import org.json.JSONObject

/**
 * Adds the mobile TUN inbound and local policy to outbounds produced by
 * libXray's share-link converter.
 */
object VpnConfigBuilder {
    fun build(
        outbounds: JSONArray,
        dpi: Boolean,
        socksPort: Int = 25346,
        directDomains: List<String> = emptyList(),
    ): String {
        require(outbounds.length() > 0) { "Нет outbound-профиля" }

        val directSettings = JSONObject().put("domainStrategy", "UseIP")
        if (dpi) {
            directSettings.put(
                "fragment",
                JSONObject()
                    .put("packets", "tlshello")
                    .put("length", "100-200")
                    .put("interval", "10-20"),
            )
        }

        val allOutbounds = JSONArray()
        for (index in 0 until outbounds.length()) {
            allOutbounds.put(outbounds.getJSONObject(index))
        }
        allOutbounds.put(
            JSONObject()
                .put("tag", "direct")
                .put("protocol", "freedom")
                .put("settings", directSettings),
        )
        allOutbounds.put(JSONObject().put("tag", "dns-out").put("protocol", "dns"))
        allOutbounds.put(JSONObject().put("tag", "block").put("protocol", "blackhole"))

        val privateIps = JSONArray()
            .put("10.0.0.0/8")
            .put("172.16.0.0/12")
            .put("192.168.0.0/16")
            .put("127.0.0.0/8")
            .put("169.254.0.0/16")
            .put("::1/128")
            .put("fc00::/7")
            .put("fe80::/10")
        val rules = JSONArray()
            .put(
                JSONObject()
                    .put("type", "field")
                    .put("port", 53)
                    .put("outboundTag", "dns-out"),
            )
        val domains = directDomains
            .map(String::trim)
            .filter { it.isNotEmpty() }
            .distinct()
            .map { if (it.startsWith("domain:") || it.startsWith("full:")) it else "domain:$it" }
        if (domains.isNotEmpty()) {
            rules.put(
                JSONObject()
                    .put("type", "field")
                    .put("domain", JSONArray(domains))
                    .put("outboundTag", "direct"),
            )
        }
        rules.put(
            JSONObject()
                .put("type", "field")
                .put("ip", privateIps)
                .put("outboundTag", "direct"),
        )

        val dnsServers = JSONArray()
            .put(
                JSONObject()
                    .put("address", "https://1.1.1.1/dns-query")
                    .put("skipFallback", false),
            )
            .put(
                JSONObject()
                    .put("address", "https://8.8.8.8/dns-query")
                    .put("skipFallback", true),
            )
        val config = JSONObject()
            .put("log", JSONObject().put("loglevel", "warning"))
            .put(
                "dns",
                JSONObject()
                    .put("servers", dnsServers)
                    .put("queryStrategy", "UseIP"),
            )
            .put(
                "inbounds",
                JSONArray().put(
                    JSONObject()
                        .put("listen", "127.0.0.1")
                        .put("port", socksPort)
                        .put("protocol", "socks")
                        .put(
                            "settings",
                            JSONObject().put("auth", "noauth").put("udp", true),
                        )
                        .put(
                            "sniffing",
                            JSONObject()
                                .put("enabled", true)
                                .put(
                                    "destOverride",
                                    JSONArray().put("http").put("tls").put("quic"),
                                )
                                // Keep the original IP so sniffing only informs
                                // routing and never triggers an internal DNS lookup.
                                .put("routeOnly", true),
                        ),
                ),
            )
            .put("outbounds", allOutbounds)
            .put(
                "routing",
                JSONObject()
                    .put("domainStrategy", "AsIs")
                    .put("rules", rules),
            )
        return config.toString()
    }
}
