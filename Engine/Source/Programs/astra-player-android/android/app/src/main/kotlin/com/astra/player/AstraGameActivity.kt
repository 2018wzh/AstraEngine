package com.astra.player

import android.content.Intent
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.hardware.input.InputManager
import android.os.Bundle
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.WindowInsets
import androidx.activity.result.contract.ActivityResultContracts
import com.google.androidgamesdk.GameActivity
import java.io.File
import java.io.FileOutputStream
import java.security.MessageDigest
import java.util.UUID
import java.util.concurrent.Executors

class AstraGameActivity : GameActivity(), AudioManager.OnAudioFocusChangeListener,
    InputManager.InputDeviceListener {
    private val packageImportExecutor = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "astra-saf-import")
    }
    private val audioManager by lazy { getSystemService(Context.AUDIO_SERVICE) as AudioManager }
    private val inputManager by lazy { getSystemService(Context.INPUT_SERVICE) as InputManager }
    private val connectedGamepads = mutableSetOf<Int>()
    private val audioFocusRequest by lazy {
        AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_GAME)
                    .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                    .build()
            )
            .setOnAudioFocusChangeListener(this)
            .setWillPauseWhenDucked(false)
            .build()
    }
    private val packagePicker = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) {
            nativeOnSafResult(null, null, 0L, false)
            return@registerForActivityResult
        }
        val flags = Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
        val persisted = runCatching {
            contentResolver.takePersistableUriPermission(uri, flags)
            true
        }.getOrDefault(false)
        if (!persisted) {
            nativeOnSafResult(null, null, 0L, false)
            return@registerForActivityResult
        }
        packageImportExecutor.execute {
            val result = runCatching { importPackage(uri) }.getOrNull()
            if (result == null) {
                nativeOnSafResult(null, null, 0L, true)
            } else {
                nativeOnSafResult(result.token, result.sha256, result.size, true)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.decorView.setOnApplyWindowInsetsListener { _, insets ->
            val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
            nativeOnInsets(bars.left, bars.top, bars.right, bars.bottom)
            insets
        }
        nativeOnRecreated(savedInstanceState != null)
        inputManager.registerInputDeviceListener(this, null)
        inputManager.inputDeviceIds.forEach(::registerGamepad)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        nativeOnNewIntent()
    }

    override fun onResume() {
        super.onResume()
        val result = audioManager.requestAudioFocus(audioFocusRequest)
        if (result != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            nativeOnAudioFocus(AudioManager.AUDIOFOCUS_LOSS)
        }
    }

    override fun onPause() {
        audioManager.abandonAudioFocusRequest(audioFocusRequest)
        nativeOnAudioFocus(AudioManager.AUDIOFOCUS_LOSS_TRANSIENT)
        super.onPause()
    }

    override fun onDestroy() {
        inputManager.unregisterInputDeviceListener(this)
        connectedGamepads.toList().forEach { deviceId ->
            nativeOnGamepadDevice(deviceId, false)
        }
        connectedGamepads.clear()
        packageImportExecutor.shutdown()
        super.onDestroy()
    }

    override fun onAudioFocusChange(focusChange: Int) {
        nativeOnAudioFocus(focusChange)
    }

    override fun onInputDeviceAdded(deviceId: Int) = registerGamepad(deviceId)

    override fun onInputDeviceRemoved(deviceId: Int) {
        if (connectedGamepads.remove(deviceId)) {
            nativeOnGamepadDevice(deviceId, false)
        }
    }

    override fun onInputDeviceChanged(deviceId: Int) {
        if (!isGamepad(inputManager.getInputDevice(deviceId))) {
            onInputDeviceRemoved(deviceId)
        } else {
            registerGamepad(deviceId)
        }
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (isGamepad(event.device)) {
            val control = when (event.keyCode) {
                KeyEvent.KEYCODE_BUTTON_A -> 0
                KeyEvent.KEYCODE_BUTTON_B -> 1
                KeyEvent.KEYCODE_BUTTON_X -> 2
                KeyEvent.KEYCODE_BUTTON_Y -> 3
                KeyEvent.KEYCODE_DPAD_UP -> 4
                KeyEvent.KEYCODE_DPAD_DOWN -> 5
                KeyEvent.KEYCODE_DPAD_LEFT -> 6
                KeyEvent.KEYCODE_DPAD_RIGHT -> 7
                KeyEvent.KEYCODE_BUTTON_L1 -> 8
                KeyEvent.KEYCODE_BUTTON_R1 -> 9
                KeyEvent.KEYCODE_BUTTON_THUMBL -> 16
                KeyEvent.KEYCODE_BUTTON_THUMBR -> 17
                KeyEvent.KEYCODE_BUTTON_START -> 18
                KeyEvent.KEYCODE_BUTTON_SELECT -> 19
                else -> -1
            }
            if (control >= 0 && event.repeatCount == 0) {
                nativeOnGamepadInput(
                    event.deviceId,
                    control,
                    if (event.action == KeyEvent.ACTION_DOWN) 1.0f else 0.0f
                )
            }
        }
        return super.dispatchKeyEvent(event)
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.action == MotionEvent.ACTION_MOVE && isGamepad(event.device)) {
            val axes = arrayOf(
                10 to MotionEvent.AXIS_LTRIGGER,
                11 to MotionEvent.AXIS_RTRIGGER,
                12 to MotionEvent.AXIS_X,
                13 to MotionEvent.AXIS_Y,
                14 to MotionEvent.AXIS_Z,
                15 to MotionEvent.AXIS_RZ,
                6 to MotionEvent.AXIS_HAT_X,
                4 to MotionEvent.AXIS_HAT_Y
            )
            axes.forEach { (control, axis) ->
                var value = event.getAxisValue(axis)
                if (axis == MotionEvent.AXIS_HAT_X || axis == MotionEvent.AXIS_HAT_Y) {
                    if (axis == MotionEvent.AXIS_HAT_X) {
                        nativeOnGamepadInput(event.deviceId, 6, if (value < -0.5f) 1.0f else 0.0f)
                        nativeOnGamepadInput(event.deviceId, 7, if (value > 0.5f) 1.0f else 0.0f)
                    } else {
                        nativeOnGamepadInput(event.deviceId, 4, if (value < -0.5f) 1.0f else 0.0f)
                        nativeOnGamepadInput(event.deviceId, 5, if (value > 0.5f) 1.0f else 0.0f)
                    }
                } else {
                    val range = event.device?.getMotionRange(axis, event.source)
                    if (range != null && kotlin.math.abs(value) <= range.flat) value = 0.0f
                    nativeOnGamepadInput(event.deviceId, control, value.coerceIn(-1.0f, 1.0f))
                }
            }
        }
        return super.dispatchGenericMotionEvent(event)
    }

    private fun registerGamepad(deviceId: Int) {
        if (isGamepad(inputManager.getInputDevice(deviceId)) && connectedGamepads.add(deviceId)) {
            nativeOnGamepadDevice(deviceId, true)
        }
    }

    private fun isGamepad(device: InputDevice?): Boolean {
        val sources = device?.sources ?: return false
        return sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD ||
            sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK
    }

    fun requestPackageDocument() {
        runOnUiThread { packagePicker.launch(arrayOf("application/octet-stream")) }
    }

    private external fun nativeOnInsets(left: Int, top: Int, right: Int, bottom: Int)
    private external fun nativeOnAudioFocus(change: Int)
    private external fun nativeOnGamepadDevice(deviceId: Int, connected: Boolean)
    private external fun nativeOnGamepadInput(deviceId: Int, control: Int, value: Float)
    private data class ImportedPackage(val token: String, val sha256: String, val size: Long)

    private fun importPackage(uri: android.net.Uri): ImportedPackage {
        val directory = File(filesDir, "package-imports")
        check(directory.exists() || directory.mkdirs()) { "package import directory is unavailable" }
        val token = "${UUID.randomUUID()}.astrapkg"
        val temporary = File(directory, "$token.part")
        val destination = File(directory, token)
        val digest = MessageDigest.getInstance("SHA-256")
        var size = 0L
        try {
            val input = checkNotNull(contentResolver.openInputStream(uri))
            input.use { source ->
                FileOutputStream(temporary).use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val count = source.read(buffer)
                        if (count < 0) break
                        if (count == 0) continue
                        size = Math.addExact(size, count.toLong())
                        check(size <= MAX_IMPORTED_PACKAGE_BYTES) { "selected package exceeds import limit" }
                        digest.update(buffer, 0, count)
                        output.write(buffer, 0, count)
                    }
                    output.fd.sync()
                }
            }
            check(size > 0L) { "selected package is empty" }
            check(temporary.renameTo(destination)) { "package import atomic rename failed" }
            val hash = digest.digest().joinToString(separator = "") { byte ->
                "%02x".format(byte.toInt() and 0xff)
            }
            return ImportedPackage(token, "sha256:$hash", size)
        } catch (error: Throwable) {
            temporary.delete()
            destination.delete()
            throw error
        }
    }

    private external fun nativeOnSafResult(token: String?, sha256: String?, size: Long, persisted: Boolean)
    private external fun nativeOnRecreated(recreated: Boolean)
    private external fun nativeOnNewIntent()

    companion object {
        private const val MAX_IMPORTED_PACKAGE_BYTES = 1024L * 1024L * 1024L

        init {
            System.loadLibrary("astra_player_android")
        }
    }
}
