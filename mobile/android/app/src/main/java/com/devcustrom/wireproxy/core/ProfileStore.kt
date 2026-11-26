package com.devcustrom.wireproxy.core

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONArray
import org.json.JSONObject

data class Profile(
    val id: Long,
    val name: String,
    val outboundsJson: String,
    val latencyMs: Long? = null,
    val sourceId: Long? = null,
)

/**
 * Profiles are stored encrypted with an Android Keystore AES-GCM key.
 * Room can replace this later, but the secret material never goes to plain
 * SharedPreferences.
 */
object ProfileStore {
    private const val PREFS = "wireproxy_profiles"
    private const val DATA = "data"
    private const val ACTIVE = "active"
    private const val KEY_ALIAS = "wireproxy_profiles_key"
    private const val IV_BYTES = 12

    private var prefs: SharedPreferences? = null
    private var nextId = 1L
    val items = mutableStateListOf<Profile>()
    var activeId by mutableStateOf<Long?>(null)
        private set

    val active: Profile?
        get() = items.firstOrNull { it.id == activeId }

    fun init(context: Context) {
        if (prefs != null) return
        prefs = context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        load()
    }

    fun add(name: String, outboundsJson: String, sourceId: Long? = null): Profile {
        val profile = Profile(nextId++, name, outboundsJson, sourceId = sourceId)
        items.add(profile)
        if (activeId == null) activeId = profile.id
        persist()
        return profile
    }

    /** Replaces all profiles that came from one subscription. */
    fun replaceSource(sourceId: Long, profiles: List<Pair<String, String>>): Int {
        val oldIds = items.filter { it.sourceId == sourceId }.map { it.id }.toSet()
        items.removeAll { it.sourceId == sourceId }
        var added = 0
        profiles.forEach { (name, outbounds) ->
            if (outbounds.isBlank()) return@forEach
            items.add(Profile(nextId++, name, outbounds, sourceId = sourceId))
            added += 1
        }
        if (activeId in oldIds) {
            activeId = items.firstOrNull { it.sourceId == sourceId }?.id
                ?: items.firstOrNull()?.id
        }
        if (activeId == null) activeId = items.firstOrNull()?.id
        persist()
        return added
    }

    fun removeSource(sourceId: Long): Int {
        val oldIds = items.filter { it.sourceId == sourceId }.map { it.id }.toSet()
        val removed = items.count { it.sourceId == sourceId }
        items.removeAll { it.sourceId == sourceId }
        if (activeId in oldIds) activeId = items.firstOrNull()?.id
        persist()
        return removed
    }

    fun select(id: Long) {
        if (items.any { it.id == id }) {
            activeId = id
            persist()
        }
    }

    fun remove(id: Long) {
        items.removeAll { it.id == id }
        if (activeId == id) activeId = items.firstOrNull()?.id
        persist()
    }

    fun setLatency(id: Long, latencyMs: Long?) {
        val index = items.indexOfFirst { it.id == id }
        if (index >= 0) {
            items[index] = items[index].copy(latencyMs = latencyMs)
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
                    Profile(
                        id = item.getLong("id"),
                        name = item.getString("name"),
                        outboundsJson = item.getString("outbounds"),
                        latencyMs = if (item.has("latency") && !item.isNull("latency")) {
                            item.getLong("latency")
                        } else {
                            null
                        },
                        sourceId = if (item.has("source") && !item.isNull("source")) {
                            item.getLong("source")
                        } else {
                            null
                        },
                    ),
                )
            }
            nextId = (items.maxOfOrNull { it.id } ?: 0L) + 1
            activeId = store.getLong(ACTIVE, 0L).takeIf { id -> items.any { it.id == id } }
        }
    }

    private fun persist() {
        val store = prefs ?: return
        val array = JSONArray()
        items.forEach { item ->
            array.put(
                JSONObject()
                    .put("id", item.id)
                    .put("name", item.name)
                    .put("outbounds", item.outboundsJson)
                    .put("latency", item.latencyMs ?: JSONObject.NULL)
                    .put("source", item.sourceId ?: JSONObject.NULL),
            )
        }
        runCatching {
            store.edit()
                .putString(DATA, encrypt(array.toString()))
                .putLong(ACTIVE, activeId ?: 0L)
                .apply()
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
