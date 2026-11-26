package com.devcustrom.wireproxy.core

import androidx.core.content.FileProvider
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class UpdateProviderInstrumentedTest {
    @Test
    fun exposesDownloadedApkThroughFileProvider() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val dir = File(context.cacheDir, "updates").apply { mkdirs() }
        val file = File(dir, "test-update.apk").apply { writeText("apk") }
        try {
            val uri = FileProvider.getUriForFile(
                context,
                "${context.packageName}.updates",
                file,
            )
            assertTrue(uri.toString().startsWith("content://${context.packageName}.updates/"))
        } finally {
            file.delete()
        }
    }
}
