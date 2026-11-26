package com.devcustrom.wireproxy.telegram

import java.io.ByteArrayOutputStream
import java.security.MessageDigest
import java.security.SecureRandom
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MtProtoCryptoTest {
    private fun sha256(data: ByteArray): ByteArray =
        MessageDigest.getInstance("SHA-256").digest(data)

    private fun clientHandshake(
        secret: ByteArray,
        dc: Int,
        media: Boolean,
        protocol: Int,
    ): Pair<ByteArray, ByteArray> {
        val random = ByteArray(64).also(SecureRandom()::nextBytes)
        random[0] = 1
        val prekeyIv = random.copyOfRange(8, 56)
        val key = sha256(prekeyIv.copyOfRange(0, 32) + secret)
        val iv = prekeyIv.copyOfRange(32, 48)
        val stream = MtProtoCrypto.Ctr(key, iv)
        stream.update(ByteArray(56))
        val tag = when (protocol) {
            MtProtoCrypto.PROTO_ABRIDGED -> byteArrayOf(-17, -17, -17, -17)
            MtProtoCrypto.PROTO_INTERMEDIATE -> byteArrayOf(-18, -18, -18, -18)
            else -> byteArrayOf(-35, -35, -35, -35)
        }
        val signedDc = if (media) -dc else dc
        val tail = ByteArrayOutputStream().apply {
            write(tag)
            write(signedDc and 0xFF)
            write((signedDc shr 8) and 0xFF)
            write(7)
            write(9)
        }.toByteArray()
        val encryptedTail = stream.update(tail)
        encryptedTail.copyInto(random, 56)
        return random to prekeyIv
    }

    @Test
    fun parsesNormalAndMediaHandshake() {
        val secret = ByteArray(16) { it.toByte() }
        val normal = clientHandshake(secret, 2, false, MtProtoCrypto.PROTO_INTERMEDIATE)
        val parsed = MtProtoCrypto.parseClientInit(normal.first, secret)!!
        assertEquals(2, parsed.dc)
        assertEquals(false, parsed.media)
        assertEquals(MtProtoCrypto.PROTO_INTERMEDIATE, parsed.protocol)

        val media = clientHandshake(secret, 4, true, MtProtoCrypto.PROTO_PADDED_INTERMEDIATE)
        val parsedMedia = MtProtoCrypto.parseClientInit(media.first, secret)!!
        assertEquals(4, parsedMedia.dc)
        assertTrue(parsedMedia.media)
        assertEquals(MtProtoCrypto.PROTO_PADDED_INTERMEDIATE, parsedMedia.protocol)
    }

    @Test
    fun rejectsWrongSecret() {
        val secret = ByteArray(16) { it.toByte() }
        val handshake = clientHandshake(secret, 2, false, MtProtoCrypto.PROTO_INTERMEDIATE)
        val wrong = ByteArray(16) { (it + 1).toByte() }
        assertEquals(null, MtProtoCrypto.parseClientInit(handshake.first, wrong))
    }

    @Test
    fun streamsInteroperateInBothDirections() {
        val secret = ByteArray(16) { (it * 3).toByte() }
        val (handshake, prekeyIv) = clientHandshake(
            secret,
            2,
            false,
            MtProtoCrypto.PROTO_INTERMEDIATE,
        )
        val init = MtProtoCrypto.parseClientInit(handshake, secret)!!
        val relayInit = MtProtoCrypto.generateRelayInit(init.protocol, 2)
        val crypto = MtProtoCrypto.buildCrypto(init, secret, relayInit)

        val plainUp = "client payload".toByteArray()
        val clientStream = MtProtoCrypto.Ctr(
            sha256(prekeyIv.copyOfRange(0, 32) + secret),
            prekeyIv.copyOfRange(32, 48),
        ).also { it.skip(64) }
        val clientCipher = clientStream.update(plainUp)
        assertArrayEquals(plainUp, crypto.clientDecrypt.update(clientCipher))

        val relayCipher = crypto.relayEncrypt.update(plainUp)
        val serverDecrypt = MtProtoCrypto.Ctr(
            relayInit.copyOfRange(8, 40),
            relayInit.copyOfRange(40, 56),
        ).also { it.skip(64) }
        assertArrayEquals(plainUp, serverDecrypt.update(relayCipher))

        val reversed = prekeyIv.reversedArray()
        val clientDecrypt = MtProtoCrypto.Ctr(
            sha256(reversed.copyOfRange(0, 32) + secret),
            reversed.copyOfRange(32, 48),
        )
        val plainDown = "server payload".toByteArray()
        assertArrayEquals(plainDown, clientDecrypt.update(
            crypto.clientEncrypt.update(plainDown),
        ))

        val relayReversed = relayInit.copyOfRange(8, 56).reversedArray()
        val serverEncrypt = MtProtoCrypto.Ctr(
            relayReversed.copyOfRange(0, 32),
            relayReversed.copyOfRange(32, 48),
        )
        assertArrayEquals(plainDown, crypto.relayDecrypt.update(
            serverEncrypt.update(plainDown),
        ))
    }

    @Test
    fun splitsIntermediatePackets() {
        val splitter = MsgSplitter(MtProtoCrypto.PROTO_INTERMEDIATE)
        val one = ByteArray(8) { 1 }
        val two = ByteArray(8) { 2 }
        val packets = ByteArrayOutputStream().apply {
            write(byteArrayOf(8, 0, 0, 0))
            write(one)
            write(byteArrayOf(8, 0, 0, 0))
            write(two)
        }.toByteArray()
        val parts = splitter.split(packets, packets)
        assertEquals(2, parts.size)
        assertEquals(12, parts[0].size)
        assertEquals(12, parts[1].size)
    }

    @Test
    fun splitsAbridgedPackets() {
        val splitter = MsgSplitter(MtProtoCrypto.PROTO_ABRIDGED)
        val packet = byteArrayOf(2, 9, 9, 9, 9, 9, 9, 9, 9)
        val parts = splitter.split(packet, packet)
        assertEquals(1, parts.size)
        assertEquals(9, parts[0].size)
    }

    @Test
    fun relayInitLooksLikeValidObfuscatedHandshake() {
        for (dc in listOf(1, 2, 4, 203, -4)) {
            val init = MtProtoCrypto.generateRelayInit(
                MtProtoCrypto.PROTO_INTERMEDIATE,
                dc,
            )
            val key = init.copyOfRange(8, 40)
            val iv = init.copyOfRange(40, 56)
            val decrypted = MtProtoCrypto.Ctr(key, iv).update(init)
            val tag = decrypted.copyOfRange(56, 60)
            assertArrayEquals(byteArrayOf(-18, -18, -18, -18), tag)
            val parsedDc = ((decrypted[60].toInt() and 0xFF) or
                ((decrypted[61].toInt() and 0xFF) shl 8)).toShort().toInt()
            assertEquals(dc, parsedDc)
        }
    }

    @Test
    fun secretHexIsFixedWidth() {
        val bytes = ByteArray(16) { (it * 17 + 128).toByte() }
        val hex = MtProtoCrypto.hex(bytes)
        assertEquals(32, hex.length)
        assertTrue(hex.all { it in "0123456789abcdef" })
    }

    @Test
    fun buffersIncompletePacket() {
        val splitter = MsgSplitter(MtProtoCrypto.PROTO_INTERMEDIATE)
        val header = byteArrayOf(8, 0, 0, 0)
        assertTrue(splitter.split(header, header).isEmpty())
        val payload = ByteArray(8) { 4 }
        val parts = splitter.split(payload, payload)
        assertEquals(1, parts.size)
        assertEquals(12, parts[0].size)
    }
}
