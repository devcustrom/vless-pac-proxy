package com.devcustrom.wireproxy.telegram

import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec

/**
 * MTProto obfuscated2 transport used by Telegram proxies.
 *
 * The client connects with `secret` embedded in the link. Both directions are
 * AES-CTR streams derived from the 64-byte client handshake; the upstream side
 * gets a second, independently generated obfuscated2 handshake so the relay
 * looks like a normal Telegram client to the DC.
 */
object MtProtoCrypto {
    const val HANDSHAKE_LEN = 64
    private const val SKIP_LEN = 8
    private const val PREKEY_LEN = 32
    private const val IV_LEN = 16
    private const val PROTO_TAG_POS = 56
    private const val DC_IDX_POS = 60

    const val PROTO_ABRIDGED = 0xEFEFEFEF.toInt()
    const val PROTO_INTERMEDIATE = 0xEEEEEEEE.toInt()
    const val PROTO_PADDED_INTERMEDIATE = 0xDDDDDDDD.toInt()

    private val TAG_ABRIDGED = byteArrayOf(0xEF.toByte(), 0xEF.toByte(), 0xEF.toByte(), 0xEF.toByte())
    private val TAG_INTERMEDIATE = byteArrayOf(0xEE.toByte(), 0xEE.toByte(), 0xEE.toByte(), 0xEE.toByte())
    private val TAG_SECURE = byteArrayOf(0xDD.toByte(), 0xDD.toByte(), 0xDD.toByte(), 0xDD.toByte())
    private val ZERO_64 = ByteArray(HANDSHAKE_LEN)
    private val random = SecureRandom()

    data class ClientInit(
        val dc: Int,
        val media: Boolean,
        val protocol: Int,
        val prekeyIv: ByteArray,
    )

    data class Crypto(
        val clientDecrypt: Ctr,
        val clientEncrypt: Ctr,
        val relayEncrypt: Ctr,
        val relayDecrypt: Ctr,
    )

    /** Parses the client handshake or returns null when the secret is wrong. */
    fun parseClientInit(handshake: ByteArray, secret: ByteArray): ClientInit? {
        if (handshake.size != HANDSHAKE_LEN) return null
        val prekeyIv = handshake.copyOfRange(SKIP_LEN, SKIP_LEN + PREKEY_LEN + IV_LEN)
        val key = sha256(prekeyIv.copyOfRange(0, PREKEY_LEN) + secret)
        val iv = prekeyIv.copyOfRange(PREKEY_LEN, PREKEY_LEN + IV_LEN)
        val decrypted = Ctr(key, iv).update(handshake)
        val tag = decrypted.copyOfRange(PROTO_TAG_POS, PROTO_TAG_POS + 4)
        val protocol = when {
            tag.contentEquals(TAG_ABRIDGED) -> PROTO_ABRIDGED
            tag.contentEquals(TAG_INTERMEDIATE) -> PROTO_INTERMEDIATE
            tag.contentEquals(TAG_SECURE) -> PROTO_PADDED_INTERMEDIATE
            else -> return null
        }
        val rawDc = (decrypted[DC_IDX_POS].toInt() and 0xFF) or
            ((decrypted[DC_IDX_POS + 1].toInt() and 0xFF) shl 8)
        val signedDc = rawDc.toShort().toInt()
        val dc = kotlin.math.abs(signedDc)
        if (dc == 0) return null
        return ClientInit(dc, signedDc < 0, protocol, prekeyIv)
    }

    fun generateRelayInit(protocol: Int, dc: Int): ByteArray {
        val rnd = ByteArray(HANDSHAKE_LEN)
        while (true) {
            random.nextBytes(rnd)
            if ((rnd[0].toInt() and 0xFF) == 0xEF) continue
            if (startsWith(rnd, byteArrayOf(0x48, 0x45, 0x41, 0x44))) continue
            if (startsWith(rnd, byteArrayOf(0x50, 0x4F, 0x53, 0x54))) continue
            if (startsWith(rnd, byteArrayOf(0x47, 0x45, 0x54, 0x20))) continue
            if (startsWith(rnd, TAG_INTERMEDIATE) || startsWith(rnd, TAG_SECURE)) continue
            if (startsWith(rnd, byteArrayOf(0x16, 0x03, 0x01, 0x02))) continue
            if (startsWith(rnd, byteArrayOf(0x4F, 0x50, 0x54, 0x49))) continue
            if (rnd[4] == 0.toByte() && rnd[5] == 0.toByte() &&
                rnd[6] == 0.toByte() && rnd[7] == 0.toByte()
            ) continue
            break
        }
        val encryptKey = rnd.copyOfRange(SKIP_LEN, SKIP_LEN + PREKEY_LEN)
        val encryptIv = rnd.copyOfRange(SKIP_LEN + PREKEY_LEN, SKIP_LEN + PREKEY_LEN + IV_LEN)
        val cipher = Ctr(encryptKey, encryptIv)
        val encryptedFull = cipher.update(rnd)
        val tag = tagFor(protocol)
        val tail = ByteArrayOutputStream().apply {
            write(tag)
            write(dc and 0xFF)
            write((dc shr 8) and 0xFF)
            write(random.nextInt(256))
            write(random.nextInt(256))
        }.toByteArray()
        for (index in 0 until 8) {
            rnd[PROTO_TAG_POS + index] = (tail[index].toInt() xor
                (encryptedFull[PROTO_TAG_POS + index].toInt() xor
                    rnd[PROTO_TAG_POS + index].toInt())).toByte()
        }
        return rnd
    }

    fun buildCrypto(clientInit: ClientInit, secret: ByteArray, relayInit: ByteArray): Crypto {
        val prekeyIv = clientInit.prekeyIv
        val clientDecKey = sha256(prekeyIv.copyOfRange(0, PREKEY_LEN) + secret)
        val clientDecIv = prekeyIv.copyOfRange(PREKEY_LEN, PREKEY_LEN + IV_LEN)
        val clientDecrypt = Ctr(clientDecKey, clientDecIv).also { it.skip(HANDSHAKE_LEN) }

        val reversed = prekeyIv.reversedArray()
        val clientEncKey = sha256(reversed.copyOfRange(0, PREKEY_LEN) + secret)
        val clientEncIv = reversed.copyOfRange(PREKEY_LEN, PREKEY_LEN + IV_LEN)
        val clientEncrypt = Ctr(clientEncKey, clientEncIv)

        val relayKey = relayInit.copyOfRange(SKIP_LEN, SKIP_LEN + PREKEY_LEN)
        val relayIv = relayInit.copyOfRange(
            SKIP_LEN + PREKEY_LEN,
            SKIP_LEN + PREKEY_LEN + IV_LEN,
        )
        val relayEncrypt = Ctr(relayKey, relayIv).also { it.skip(HANDSHAKE_LEN) }

        val relayReversed = relayInit.copyOfRange(
            SKIP_LEN,
            SKIP_LEN + PREKEY_LEN + IV_LEN,
        ).reversedArray()
        val relayDecrypt = Ctr(
            relayReversed.copyOfRange(0, PREKEY_LEN),
            relayReversed.copyOfRange(PREKEY_LEN, PREKEY_LEN + IV_LEN),
        )
        return Crypto(clientDecrypt, clientEncrypt, relayEncrypt, relayDecrypt)
    }

    fun protocolName(protocol: Int): String = when (protocol) {
        PROTO_ABRIDGED -> "abridged"
        PROTO_INTERMEDIATE -> "intermediate"
        else -> "padded"
    }

    fun hex(bytes: ByteArray): String =
        bytes.joinToString("") { "%02x".format(it.toInt() and 0xFF) }

    private fun tagFor(protocol: Int): ByteArray = when (protocol) {
        PROTO_ABRIDGED -> TAG_ABRIDGED
        PROTO_INTERMEDIATE -> TAG_INTERMEDIATE
        else -> TAG_SECURE
    }

    private fun startsWith(data: ByteArray, prefix: ByteArray): Boolean {
        if (data.size < prefix.size) return false
        for (index in prefix.indices) {
            if (data[index] != prefix[index]) return false
        }
        return true
    }

    private fun sha256(data: ByteArray): ByteArray =
        MessageDigest.getInstance("SHA-256").digest(data)

    class Ctr(key: ByteArray, iv: ByteArray) {
        private val cipher = Cipher.getInstance("AES/CTR/NoPadding").apply {
            init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), IvParameterSpec(iv))
        }

        fun update(data: ByteArray): ByteArray = cipher.update(data)

        fun skip(count: Int) {
            cipher.update(ZERO_64.copyOf(count))
        }
    }
}

/**
 * Splits the re-encrypted TCP stream into original MTProto packets so the
 * WebSocket side gets one message per packet, as the Telegram WS gateway
 * expects.
 */
class MsgSplitter(private val protocol: Int) {
    private var cipherBuf = ByteArray(0)
    private var plainBuf = ByteArray(0)
    private var disabled = false

    fun split(cipher: ByteArray, plain: ByteArray): List<ByteArray> {
        if (cipher.isEmpty()) return emptyList()
        if (disabled) return listOf(cipher)
        cipherBuf += cipher
        plainBuf += plain
        val parts = mutableListOf<ByteArray>()
        var offset = 0
        while (offset < cipherBuf.size) {
            val length = nextPacketLength(offset, plainBuf.size - offset) ?: break
            if (length <= 0) {
                parts += cipherBuf.copyOfRange(offset, cipherBuf.size)
                offset = cipherBuf.size
                disabled = true
                break
            }
            parts += cipherBuf.copyOfRange(offset, offset + length)
            offset += length
        }
        if (offset > 0) {
            cipherBuf = cipherBuf.copyOfRange(offset, cipherBuf.size)
            plainBuf = plainBuf.copyOfRange(offset, plainBuf.size)
        }
        return parts
    }

    fun flush(): ByteArray? {
        if (cipherBuf.isEmpty()) return null
        val tail = cipherBuf
        cipherBuf = ByteArray(0)
        plainBuf = ByteArray(0)
        return tail
    }

    private fun nextPacketLength(offset: Int, available: Int): Int? {
        if (available <= 0) return null
        return when (protocol) {
            MtProtoCrypto.PROTO_ABRIDGED -> {
                val first = plainBuf[offset].toInt() and 0xFF
                if (first == 0x7F || first == 0xFF) {
                    if (available < 4) return null
                    val payload = ((plainBuf[offset + 1].toInt() and 0xFF) or
                        ((plainBuf[offset + 2].toInt() and 0xFF) shl 8) or
                        ((plainBuf[offset + 3].toInt() and 0xFF) shl 16)) * 4
                    if (payload <= 0) return 0
                    val total = 4 + payload
                    return if (available < total) null else total
                } else {
                    val payload = (first and 0x7F) * 4
                    if (payload <= 0) return 0
                    val total = 1 + payload
                    return if (available < total) null else total
                }
            }
            MtProtoCrypto.PROTO_INTERMEDIATE,
            MtProtoCrypto.PROTO_PADDED_INTERMEDIATE,
            -> {
                if (available < 4) return null
                val payload = ((plainBuf[offset].toInt() and 0xFF) or
                    ((plainBuf[offset + 1].toInt() and 0xFF) shl 8) or
                    ((plainBuf[offset + 2].toInt() and 0xFF) shl 16) or
                    ((plainBuf[offset + 3].toInt() and 0xFF) shl 24)) and 0x7FFFFFFF
                if (payload <= 0) return 0
                val total = 4 + payload
                return if (available < total) null else total
            }
            else -> 0
        }
    }
}
