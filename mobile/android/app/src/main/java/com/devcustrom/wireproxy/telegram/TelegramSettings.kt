package com.devcustrom.wireproxy.telegram

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Small encrypted preference store for the MTProto proxy secret and the last
 * working CF route. The secret is never written in plaintext and never logged.
 */
object TelegramSettings {
    private const val PREFS = "telegram_proxy"
    private const val SECRET = "secret"
    private const val LAST_ROUTE = "last_route"
    private const val KEY_ALIAS = "wireproxy_telegram_key"
    private const val IV_BYTES = 12

    private var prefs: SharedPreferences? = null

    fun init(context: Context) {
        if (prefs == null) {
            prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        }
    }

    fun secret(): ByteArray {
        val store = prefs ?: return randomSecret()
        val encoded = store.getString(SECRET, null)
        if (encoded != null) {
            val decoded = runCatching { decrypt(encoded) }.getOrNull()
            if (decoded != null && decoded.size == 16) return decoded
        }
        val generated = randomSecret()
        runCatching { store.edit().putString(SECRET, encrypt(generated)).apply() }
        return generated
    }

    var lastRoute: String?
        get() = prefs?.getString(LAST_ROUTE, null)
        set(value) {
            prefs?.edit()?.putString(LAST_ROUTE, value)?.apply()
        }

    fun link(port: Int, host: String = "127.0.0.1"): String {
        val hex = MtProtoCrypto.hex(secret())
        return "tg://proxy?server=$host&port=$port&secret=dd$hex"
    }

    fun maskedSecret(): String {
        val hex = MtProtoCrypto.hex(secret())
        return hex.take(4) + "…" + hex.takeLast(4)
    }

    private fun randomSecret(): ByteArray = ByteArray(16).also(SecureRandom()::nextBytes)

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getEntry(KEY_ALIAS, null) as? KeyStore.SecretKeyEntry)?.let {
            return it.secretKey
        }
        val generator = KeyGenerator.getInstance(
            KeyProperties.KEY_ALGORITHM_AES,
            "AndroidKeyStore",
        )
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return generator.generateKey()
    }

    private fun encrypt(plain: ByteArray): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val iv = cipher.iv
        val encrypted = cipher.doFinal(plain)
        val packed = ByteArray(iv.size + encrypted.size)
        System.arraycopy(iv, 0, packed, 0, iv.size)
        System.arraycopy(encrypted, 0, packed, iv.size, encrypted.size)
        return Base64.encodeToString(packed, Base64.NO_WRAP)
    }

    private fun decrypt(encoded: String): ByteArray {
        val packed = Base64.decode(encoded, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(
            Cipher.DECRYPT_MODE,
            key(),
            GCMParameterSpec(128, packed.copyOfRange(0, IV_BYTES)),
        )
        return cipher.doFinal(packed.copyOfRange(IV_BYTES, packed.size))
    }
}
