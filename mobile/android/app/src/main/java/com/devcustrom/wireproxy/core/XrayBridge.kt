package com.devcustrom.wireproxy.core

import android.net.VpnService
import android.util.Log
import libXray.DialerController
import libXray.LibXray
import org.json.JSONArray
import org.json.JSONObject

/**
 * Thin Kotlin boundary around libXray's JSON API.
 *
 * libXray owns the Xray-core runtime. The TUN fd is passed through the Xray
 * config root `env` object as `xray.tun.fd`; outbound sockets are protected
 * through VpnService so they do not loop back into the TUN.
 */
object XrayBridge {
    data class Result(
        val success: Boolean,
        val data: JSONObject?,
        val error: String,
    )

    data class PingResult(
        val success: Boolean,
        val delayMs: Long,
        val error: String,
    )

    private var controller: DialerController? = null

    private fun invoke(method: String, payload: JSONObject = JSONObject()): Result {
        return try {
            val request = JSONObject()
                .put("apiVersion", 3)
                .put("method", method)
                .put("payload", payload)
            val response = JSONObject(LibXray.invoke(request.toString()))
            val result = Result(
                success = response.optBoolean("success", false),
                data = response.optJSONObject("data"),
                error = response.optString("error"),
            )
            if (!result.success) {
                Log.e("XrayBridge", "$method: ${result.error}")
            }
            result
        } catch (error: Throwable) {
            Log.e("XrayBridge", "$method failed", error)
            Result(false, null, error.message ?: error.javaClass.simpleName)
        }
    }

    fun version(): String {
        val result = invoke("xrayVersion")
        return result.data?.optString("version")
            ?.takeIf { it.isNotBlank() }
            ?: result.error.ifBlank { "Xray не ответил" }
    }

    fun convertShareLinks(text: String): Result =
        invoke("convertShareLinksToXrayJson", JSONObject().put("text", text))

    fun test(xrayJson: String): Result =
        invoke("testXray", JSONObject().put("xrayJson", xrayJson))

    fun pingBatch(
        configs: List<String>,
        url: String = "https://cp.cloudflare.com/",
        timeoutSeconds: Int = 8,
    ): List<PingResult> {
        val entries = JSONArray()
        configs.forEach { config ->
            entries.put(JSONObject().put("xrayJson", config))
        }
        val payload = JSONObject()
            .put("configs", entries)
            .put("timeout", timeoutSeconds)
            .put("url", url)
            .put("warm", true)
        val request = JSONObject()
            .put("apiVersion", 3)
            .put("method", "pingBatch")
            .put("payload", payload)
        val response = JSONObject(LibXray.invoke(request.toString()))
        if (!response.optBoolean("success", false)) {
            throw IllegalStateException(
                response.optString("error").ifBlank { "pingBatch не выполнился" },
            )
        }
        val data = response.opt("data")
        val items = when (data) {
            is JSONArray -> data
            is JSONObject -> data.optJSONArray("results") ?: JSONArray()
            else -> JSONArray()
        }
        return (0 until items.length()).map { index ->
            val item = items.optJSONObject(index) ?: JSONObject()
            PingResult(
                success = item.optBoolean("success", false),
                delayMs = item.optLong("delay", 10_000L),
                error = item.optString("error"),
            )
        }
    }

    fun run(xrayJson: String): Result =
        invoke("runXray", JSONObject().put("xrayJson", xrayJson))

    fun run(xrayJson: String, tunFd: Int): Result {
        val config = JSONObject(xrayJson)
        val env = config.optJSONObject("env") ?: JSONObject()
        env.put("xray.tun.fd", tunFd.toString())
        config.put("env", env)
        return invoke("runXray", JSONObject().put("xrayJson", config.toString()))
    }

    fun stop(): Result = invoke("stopXray")

    fun attachVpn(vpn: VpnService, dnsServer: String = "1.1.1.1:53") {
        val next = object : DialerController {
            override fun protectFd(fd: Long): Boolean = vpn.protect(fd.toInt())
        }
        controller = next
        LibXray.registerDialerController(next)
        runCatching { LibXray.setDNS(next, dnsServer) }
    }

    fun resetDns() {
        runCatching { LibXray.resetDNS() }
    }
}
