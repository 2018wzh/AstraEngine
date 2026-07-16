package org.astraemu.manager

import android.app.Activity
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.util.ArrayDeque
import java.util.HashSet
import java.security.KeyStore
import java.security.MessageDigest
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

object AstraPlatformBridge {
    private const val SECRET_PREFERENCES = "astraemu-secure-references-v1"

    @JvmStatic
    fun packageIdentity(activity: Activity): ByteArray {
        val packageName = activity.packageName
        val packageManager = activity.packageManager
        val packageInfo = if (Build.VERSION.SDK_INT >= 28) {
            packageManager.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES)
        } else {
            @Suppress("DEPRECATION")
            packageManager.getPackageInfo(packageName, PackageManager.GET_SIGNATURES)
        }
        val signatures = if (Build.VERSION.SDK_INT >= 28) {
            val signingInfo = packageInfo.signingInfo
                ?: throw SecurityException("missing APK signing identity")
            if (signingInfo.hasMultipleSigners()) signingInfo.apkContentsSigners
            else signingInfo.signingCertificateHistory
        } else {
            @Suppress("DEPRECATION")
            packageInfo.signatures
        }
        require(signatures != null && signatures.size == 1) {
            "AstraEMU requires exactly one APK signer identity"
        }
        val versionCode = if (Build.VERSION.SDK_INT >= 28) {
            packageInfo.longVersionCode
        } else {
            @Suppress("DEPRECATION")
            packageInfo.versionCode.toLong()
        }
        return encode(maxBytes = 128 * 1024) {
            write("ASTI1".toByteArray(Charsets.US_ASCII))
            writeSized(packageName.toByteArray(Charsets.UTF_8))
            writeLong(versionCode)
            writeSized(signatures[0].toByteArray())
            writeSized(activity.applicationInfo.nativeLibraryDir.toByteArray(Charsets.UTF_8))
            writeSized(activity.filesDir.absolutePath.toByteArray(Charsets.UTF_8))
            writeInt(Build.VERSION.SDK_INT)
        }
    }

    @JvmStatic
    fun enumerateTree(
        activity: Activity,
        treeUriText: String,
        maxEntries: Int,
        maxEncodedBytes: Int,
    ): ByteArray {
        require(maxEntries in 1..100_000)
        require(maxEncodedBytes in 1024..(32 * 1024 * 1024))
        val treeUri = Uri.parse(treeUriText)
        require(treeUri.scheme == "content")
        val rootId = DocumentsContract.getTreeDocumentId(treeUri)
        val pending = ArrayDeque<Pair<String, String>>()
        val visitedDirectories = HashSet<String>()
        val files = ArrayList<DocumentRecord>()
        pending.add(rootId to "")
        while (pending.isNotEmpty()) {
            val (parentId, parentPath) = pending.removeFirst()
            check(visitedDirectories.add(parentId)) { "document provider directory cycle" }
            val children = DocumentsContract.buildChildDocumentsUriUsingTree(treeUri, parentId)
            activity.contentResolver.query(children, PROJECTION, null, null, null).use { cursor ->
                requireNotNull(cursor) { "document provider returned no cursor" }
                while (cursor.moveToNext()) {
                    val id = cursor.requiredString(DOCUMENT_ID)
                    val name = cursor.requiredString(DISPLAY_NAME)
                    validatePathPart(name)
                    val path = if (parentPath.isEmpty()) name else "$parentPath/$name"
                    val mime = cursor.requiredString(MIME_TYPE)
                    if (mime == DocumentsContract.Document.MIME_TYPE_DIR) {
                        pending.add(id to path)
                    } else {
                        val uri = DocumentsContract.buildDocumentUriUsingTree(treeUri, id)
                        files.add(
                            DocumentRecord(
                                path,
                                uri.toString(),
                                cursor.optionalLong(LAST_MODIFIED),
                                cursor.optionalLong(SIZE).coerceAtLeast(0),
                            )
                        )
                        check(files.size <= maxEntries) { "document entry bound exceeded" }
                    }
                }
            }
        }
        files.sortBy { it.path.lowercase() }
        for (index in 1 until files.size) {
            check(!files[index - 1].path.equals(files[index].path, ignoreCase = true)) {
                "case-insensitive document path collision"
            }
        }
        return encode(maxEncodedBytes) {
            write("ASTS1".toByteArray(Charsets.US_ASCII))
            writeInt(files.size)
            files.forEach { file ->
                writeSized(file.path.toByteArray(Charsets.UTF_8))
                writeSized(file.uri.toByteArray(Charsets.UTF_8))
                writeLong(file.modifiedMs)
                writeLong(file.byteSize)
            }
        }
    }

    @JvmStatic
    fun readDocument(activity: Activity, documentUriText: String, maxBytes: Int): ByteArray {
        require(maxBytes in 0..Int.MAX_VALUE - 1)
        val uri = Uri.parse(documentUriText)
        require(uri.scheme == "content")
        activity.contentResolver.openInputStream(uri).use { input ->
            requireNotNull(input) { "document provider returned no input stream" }
            val output = ByteArrayOutputStream(minOf(maxBytes, 1024 * 1024))
            val buffer = ByteArray(64 * 1024)
            var total = 0
            while (true) {
                val read = input.read(buffer)
                if (read < 0) break
                total = Math.addExact(total, read)
                check(total <= maxBytes) { "document byte bound exceeded" }
                output.write(buffer, 0, read)
            }
            return output.toByteArray()
        }
    }

    @JvmStatic
    fun storeSecret(activity: Activity, reference: String, secret: String): String {
        validateSecretReference(reference)
        require(secret.isNotEmpty() && secret.toByteArray(Charsets.UTF_8).size <= 16 * 1024)
        val key = secretKey(reference, create = true)
            ?: throw SecurityException("failed to create Android Keystore key")
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val encrypted = cipher.doFinal(secret.toByteArray(Charsets.UTF_8))
        val encoded = Base64.encodeToString(cipher.iv, Base64.NO_WRAP) + "." +
            Base64.encodeToString(encrypted, Base64.NO_WRAP)
        val committed = activity.getSharedPreferences(SECRET_PREFERENCES, Activity.MODE_PRIVATE)
            .edit()
            .putString(reference, encoded)
            .commit()
        check(committed) { "secure reference persistence failed" }
        return ""
    }

    @JvmStatic
    fun resolveSecret(activity: Activity, reference: String): String {
        validateSecretReference(reference)
        val encoded = activity.getSharedPreferences(SECRET_PREFERENCES, Activity.MODE_PRIVATE)
            .getString(reference, null)
            ?: throw SecurityException("secure reference is missing")
        val parts = encoded.split('.', limit = 2)
        require(parts.size == 2)
        val iv = Base64.decode(parts[0], Base64.NO_WRAP)
        val encrypted = Base64.decode(parts[1], Base64.NO_WRAP)
        require(iv.size in 12..16 && encrypted.size <= 16 * 1024 + 32)
        val key = secretKey(reference, create = false)
            ?: throw SecurityException("Android Keystore key is missing")
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, iv))
        return cipher.doFinal(encrypted).toString(Charsets.UTF_8)
    }

    private fun secretKey(reference: String, create: Boolean): SecretKey? {
        val alias = "astraemu." + Base64.encodeToString(
            MessageDigest.getInstance("SHA-256").digest(reference.toByteArray(Charsets.UTF_8)),
            Base64.NO_WRAP or Base64.NO_PADDING or Base64.URL_SAFE,
        )
        val keyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (keyStore.getKey(alias, null) as? SecretKey)?.let { return it }
        if (!create) return null
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }

    private fun validateSecretReference(reference: String) {
        require(reference.isNotEmpty() && reference.length <= 128)
        require(reference.all { it.isLetterOrDigit() || it == '.' || it == '-' || it == '_' })
    }

    private inline fun encode(maxBytes: Int, block: BoundedDataOutput.() -> Unit): ByteArray {
        val bytes = ByteArrayOutputStream(minOf(maxBytes, 1024 * 1024))
        val output = BoundedDataOutput(DataOutputStream(bytes), bytes, maxBytes)
        output.block()
        output.flush()
        return bytes.toByteArray()
    }

    private fun validatePathPart(value: String) {
        require(value.isNotEmpty() && value.length <= 255)
        require(value != "." && value != "..")
        require(!value.contains('/') && !value.contains('\\') && !value.contains('\u0000'))
    }

    private fun Cursor.requiredString(column: String): String {
        val index = getColumnIndex(column)
        require(index >= 0 && !isNull(index)) { "missing document column" }
        return getString(index)
    }

    private fun Cursor.optionalLong(column: String): Long {
        val index = getColumnIndex(column)
        return if (index < 0 || isNull(index)) 0 else getLong(index)
    }

    private data class DocumentRecord(
        val path: String,
        val uri: String,
        val modifiedMs: Long,
        val byteSize: Long,
    )

    private class BoundedDataOutput(
        private val output: DataOutputStream,
        private val bytes: ByteArrayOutputStream,
        private val maxBytes: Int,
    ) {
        fun write(value: ByteArray) {
            ensure(value.size)
            output.write(value)
        }

        fun writeSized(value: ByteArray) {
            ensure(Math.addExact(4, value.size))
            output.writeInt(value.size)
            output.write(value)
        }

        fun writeInt(value: Int) {
            ensure(4)
            output.writeInt(value)
        }

        fun writeLong(value: Long) {
            ensure(8)
            output.writeLong(value)
        }

        fun flush() = output.flush()

        private fun ensure(additional: Int) {
            check(Math.addExact(bytes.size(), additional) <= maxBytes) {
                "encoded payload bound exceeded"
            }
        }
    }

    private val DOCUMENT_ID = DocumentsContract.Document.COLUMN_DOCUMENT_ID
    private val DISPLAY_NAME = DocumentsContract.Document.COLUMN_DISPLAY_NAME
    private val MIME_TYPE = DocumentsContract.Document.COLUMN_MIME_TYPE
    private val LAST_MODIFIED = DocumentsContract.Document.COLUMN_LAST_MODIFIED
    private val SIZE = DocumentsContract.Document.COLUMN_SIZE
    private val PROJECTION = arrayOf(DOCUMENT_ID, DISPLAY_NAME, MIME_TYPE, LAST_MODIFIED, SIZE)
}
