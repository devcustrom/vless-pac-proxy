package com.devcustrom.wireproxy.core

import android.content.Context
import android.util.Log
import java.io.File
import java.net.InetSocketAddress
import java.net.Socket

/**
 * Runs the bundled ByeDPI (ciadpi) SOCKS5 proxy for no-root DPI bypass.
 *
 * The executable is packaged as `libciadpi.so` so Android extracts it into the
 * app's native library directory, where executing it is permitted. It listens
 * on loopback only.
 */
object ByeDpi {
    const val PORT = 1081
    private const val TAG = "ByeDpi"
    private var process: Process? = null
    private var refs = 0

    @Volatile
    var lastError: String? = null
        private set

    val running: Boolean
        get() = process?.isAlive == true

    /** Starts ByeDPI (or reuses it) and takes one reference. */
    @Synchronized
    fun start(context: Context, mode: String = "medium"): Boolean {
        if (running) {
            refs += 1
            return true
        }
        val binary = File(context.applicationInfo.nativeLibraryDir, "libciadpi.so")
        if (!binary.isFile) {
            lastError = "ByeDPI-бинарник не найден"
            return false
        }
        val args = mutableListOf(
            binary.absolutePath,
            "-i", "127.0.0.1",
            "-I", "0.0.0.0",
            "-p", PORT.toString(),
            "-x", "0",
            "-U",
        )
        args += strategy(mode)
        return try {
            process = ProcessBuilder(args)
                .redirectErrorStream(true)
                .start()
            if (waitUntilReady()) {
                refs = 1
                true
            } else {
                stopProcess()
                false
            }
        } catch (error: Throwable) {
            lastError = error.message ?: "Не удалось запустить ByeDPI"
            Log.e(TAG, "start failed", error)
            stopProcess()
            false
        }
    }

    /** Releases one reference; the process stops after the last user. */
    @Synchronized
    fun stop() {
        if (refs > 0) refs -= 1
        if (refs == 0) stopProcess()
    }

    @Synchronized
    private fun stopProcess() {
        refs = 0
        val current = process ?: return
        process = null
        runCatching { current.destroy() }
        runCatching {
            if (!current.waitFor(2, java.util.concurrent.TimeUnit.SECONDS)) {
                current.destroyForcibly()
            }
        }
    }

    private fun waitUntilReady(): Boolean {
        val deadline = System.currentTimeMillis() + 2500
        while (System.currentTimeMillis() < deadline) {
            if (process?.isAlive != true) {
                lastError = "ByeDPI завершился при запуске"
                return false
            }
            runCatching {
                Socket().use { socket ->
                    socket.connect(InetSocketAddress("127.0.0.1", PORT), 150)
                    return true
                }
            }
            Thread.sleep(100)
        }
        lastError = "ByeDPI не открыл порт"
        return false
    }

    private fun strategy(mode: String): List<String> = when (mode) {
        "light" -> listOf("--disorder", "1")
        "aggressive" -> listOf(
            "--split", "1+s",
            "--disorder", "3+s",
            "--auto=torst",
        )
        else -> listOf(
            "--split", "1+s",
            "--disorder", "1+s",
            "--auto=torst",
        )
    }
}
