package com.devcustrom.wireproxy.telegram

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.InetSocketAddress
import java.net.Socket
import java.security.MessageDigest
import java.security.SecureRandom

/**
 * Minimal MTProto obfuscated2 client for tests. Sends an unencrypted
 * `req_pq_multi` and returns the constructor of the first response packet.
 */
object MtProtoTestClient {
    private const val RES_PQ = 0x05162463
    fun reqPq(
        port: Int,
        secret: ByteArray,
        dc: Int = 2,
        media: Boolean = false,
        timeoutMs: Int = 40_000,
    ): Int {
        Socket().use { socket ->
            socket.connect(InetSocketAddress("127.0.0.1", port), 5_000)
            socket.soTimeout = timeoutMs
            val input = socket.getInputStream()
            val output = socket.getOutputStream()
            val keys = clientHandshake(secret, dc, media)
            output.write(keys.handshake)
            output.flush()
            output.write(keys.encrypt.update(reqPqMulti()))
            output.flush()

            val encryptedLength = readExactly(input, 4)
                ?: error("no response from proxy")
            val lengthPlain = keys.decrypt.update(encryptedLength)
            val length = (lengthPlain[0].toInt() and 0xFF) or
                ((lengthPlain[1].toInt() and 0xFF) shl 8) or
                ((lengthPlain[2].toInt() and 0xFF) shl 16) or
                ((lengthPlain[3].toInt() and 0xFF) shl 24)
            require(length in 16..1_000_000) { "unexpected frame length $length" }
            val encryptedBody = readExactly(input, length)
                ?: error("incomplete response")
            val payload = keys.decrypt.update(encryptedBody)
            return (payload[20].toInt() and 0xFF) or
                ((payload[21].toInt() and 0xFF) shl 8) or
                ((payload[22].toInt() and 0xFF) shl 16) or
                ((payload[23].toInt() and 0xFF) shl 24)
        }
    }

    fun isResPq(constructor: Int): Boolean = constructor == RES_PQ

    private class ClientKeys(
        val handshake: ByteArray,
        val encrypt: MtProtoCrypto.Ctr,
        val decrypt: MtProtoCrypto.Ctr,
    )

    private fun clientHandshake(secret: ByteArray, dc: Int, media: Boolean): ClientKeys {
        val random = ByteArray(64).also(SecureRandom()::nextBytes)
        random[0] = 0x37
        val prekeyIv = random.copyOfRange(8, 56)
        val key = MessageDigest.getInstance("SHA-256")
            .digest(prekeyIv.copyOfRange(0, 32) + secret)
        val iv = prekeyIv.copyOfRange(32, 48)
        val tailStream = MtProtoCrypto.Ctr(key, iv)
        tailStream.update(ByteArray(56))
        val signedDc = if (media) -dc else dc
        val tail = ByteArrayOutputStream().apply {
            write(byteArrayOf(0xEE.toByte(), 0xEE.toByte(), 0xEE.toByte(), 0xEE.toByte()))
            write(signedDc and 0xFF)
            write((signedDc shr 8) and 0xFF)
            write(1)
            write(2)
        }.toByteArray()
        tailStream.update(tail).copyInto(random, 56)

        val encrypt = MtProtoCrypto.Ctr(key, iv).also { it.skip(64) }
        val reversed = prekeyIv.reversedArray()
        val decryptKey = MessageDigest.getInstance("SHA-256")
            .digest(reversed.copyOfRange(0, 32) + secret)
        val decrypt = MtProtoCrypto.Ctr(decryptKey, reversed.copyOfRange(32, 48))
        return ClientKeys(random, encrypt, decrypt)
    }

    private fun reqPqMulti(): ByteArray {
        val nonce = ByteArray(16).also(SecureRandom()::nextBytes)
        val body = ByteArrayOutputStream().apply {
            write(byteArrayOf(0xF1.toByte(), 0x8E.toByte(), 0x7E.toByte(), 0xBE.toByte()))
            write(nonce)
        }.toByteArray()
        val message = ByteArrayOutputStream().apply {
            write(ByteArray(8))
            val seconds = System.currentTimeMillis() / 1000
            val msgId = seconds shl 32
            for (shift in 56 downTo 0 step 8) {
                write(((msgId shr shift) and 0xFF).toInt())
            }
            write(byteArrayOf(
                (body.size and 0xFF).toByte(),
                ((body.size shr 8) and 0xFF).toByte(),
                ((body.size shr 16) and 0xFF).toByte(),
                ((body.size shr 24) and 0xFF).toByte(),
            ))
            write(body)
        }.toByteArray()
        return ByteArrayOutputStream().apply {
            write(byteArrayOf(
                (message.size and 0xFF).toByte(),
                ((message.size shr 8) and 0xFF).toByte(),
                ((message.size shr 16) and 0xFF).toByte(),
                ((message.size shr 24) and 0xFF).toByte(),
            ))
            write(message)
        }.toByteArray()
    }

    private fun readExactly(input: InputStream, size: Int): ByteArray? {
        val data = ByteArray(size)
        var offset = 0
        while (offset < size) {
            val read = input.read(data, offset, size - offset)
            if (read < 0) return null
            offset += read
        }
        return data
    }
}
