package com.devcustrom.wireproxy.core

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.compose.runtime.mutableStateListOf
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONArray
import org.json.JSONObject

data class Subscription(
    val id: Long,
    val url: String,
    val name: String,
    val updatedAt: Long = 0,
    val lastError: String? = null,
)

/**
 * Subscription URLs often carry access tokens, so they are stored with the
 * same Android Keystore AES-GCM scheme as profiles.
 */
object SubscriptionStore {
    private const val PREFS = "wireproxy_subscriptions"
    private const val DATA = "data"
    private const val KEY_ALIAS = "wireproxy_subscriptions_key"
    private const val IV_BYTES = 12

    private var prefs: SharedPreferences? = null
    private var nextId = 1L
    val items = mutableStateListOf<Subscription>()

    fun init(context: Context) {
        if (prefs != null) return
        prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        load()
    }

    fun add(url: String, name: String): Subscription {
        val subscription = Subscription(nextId++, url, name)
        items.add(subscription)
        persist()
        return subscription
    }

    fun remove(id: Long) {
        items.removeAll { it.id == id }
        persist()
    }

    fun update(id: Long, updatedAt: Long, lastError: String?) {
        val index = items.indexOfFirst { it.id == id }
        if (index >= 0) {
            items[index] = items[index].copy(updatedAt = updatedAt, lastError = lastError)
            persist()
        }
    }

    private fun load() {
        val store = prefs ?: return
        val encoded = store.getString(DATA, null) ?: return
        val plain = runCatching { decrypt(encoded) }.getOrNull() ?: return
        runCatching {
            val array = JSONArray(plain)
            for (index in 0 until array.length()) {
                val item = array.getJSONObject(index)
                items.add(
                    Subscription(
                        id = item.getLong("id"),
                        url = item.getString("url"),
                        name = item.getString("name"),
                        updatedAt = item.optLong("updated", 0),
                        lastError = item.optString("error").ifBlank { null },
                    ),
                )
            }
            nextId = (items.maxOfOrNull { it.id } ?: 0L) + 1
        }
    }

    private fun persist() {
        val store = prefs ?: return
        val array = JSONArray()
        items.forEach { item ->
            array.put(
                JSONObject()
                    .put("id", item.id)
                    .put("url", item.url)
                    .put("name", item.name)
                    .put("updated", item.updatedAt)
                    .put("error", item.lastError ?: ""),
            )
        }
        runCatching {
            store.edit().putString(DATA, encrypt(array.toString())).apply()
        }
    }

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

    private fun encrypt(plain: String): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val iv = cipher.iv
        val encrypted = cipher.doFinal(plain.toByteArray(Charsets.UTF_8))
        val packed = ByteArray(iv.size + encrypted.size)
        System.arraycopy(iv, 0, packed, 0, iv.size)
        System.arraycopy(encrypted, 0, packed, iv.size, encrypted.size)
        return Base64.encodeToString(packed, Base64.NO_WRAP)
    }

    private fun decrypt(encoded: String): String {
        val packed = Base64.decode(encoded, Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(
            Cipher.DECRYPT_MODE,
            key(),
            GCMParameterSpec(128, packed.copyOfRange(0, IV_BYTES)),
        )
        val plain = cipher.doFinal(packed.copyOfRange(IV_BYTES, packed.size))
        return String(plain, Charsets.UTF_8)
    }
}
