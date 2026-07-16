package org.astraemu.manager

import android.app.NativeActivity
import android.content.Intent
import android.content.pm.ActivityInfo
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.Uri
import android.os.Bundle
import android.view.InputDevice
import android.view.KeyEvent
import android.view.MotionEvent

class MainActivity : NativeActivity() {
    private val treeRequestCode = 0xA57A
    private lateinit var audioManager: AudioManager
    private var audioFocusRequest: AudioFocusRequest? = null

    external fun nativeOnDocumentTreeGranted(uri: String)
    external fun nativeOnLifecycleChanged(state: Int)
    external fun nativeOnGamepadInput(control: String, pressed: Boolean, value: Float)

    private var leftPressed = false
    private var rightPressed = false
    private var upPressed = false
    private var downPressed = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        audioManager = getSystemService(AudioManager::class.java)
    }

    fun requestDocumentTree() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION or
                    Intent.FLAG_GRANT_PREFIX_URI_PERMISSION
            )
        }
        startActivityForResult(intent, treeRequestCode)
    }

    fun setGameMode(enabled: Boolean) {
        val compactPhone = resources.configuration.smallestScreenWidthDp < 600
        requestedOrientation = if (enabled && compactPhone) {
            ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        } else {
            ActivityInfo.SCREEN_ORIENTATION_UNSPECIFIED
        }
        if (enabled) requestGameAudioFocus() else abandonGameAudioFocus()
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != treeRequestCode || resultCode != RESULT_OK) return
        val uri = data?.data ?: return
        val flags = data.flags and
            (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        contentResolver.takePersistableUriPermission(uri, flags and Intent.FLAG_GRANT_READ_URI_PERMISSION)
        nativeOnDocumentTreeGranted(uri.toString())
    }

    override fun onResume() {
        super.onResume()
        nativeOnLifecycleChanged(1)
    }

    override fun onPause() {
        nativeOnLifecycleChanged(0)
        super.onPause()
    }

    override fun dispatchKeyEvent(event: KeyEvent): Boolean {
        if (event.source and InputDevice.SOURCE_GAMEPAD != InputDevice.SOURCE_GAMEPAD) {
            return super.dispatchKeyEvent(event)
        }
        val control = when (event.keyCode) {
            KeyEvent.KEYCODE_BUTTON_A, KeyEvent.KEYCODE_BUTTON_START -> "confirm"
            KeyEvent.KEYCODE_BUTTON_B, KeyEvent.KEYCODE_BUTTON_SELECT -> "cancel"
            KeyEvent.KEYCODE_DPAD_UP -> "up"
            KeyEvent.KEYCODE_DPAD_DOWN -> "down"
            KeyEvent.KEYCODE_DPAD_LEFT -> "left"
            KeyEvent.KEYCODE_DPAD_RIGHT -> "right"
            else -> return super.dispatchKeyEvent(event)
        }
        when (event.action) {
            KeyEvent.ACTION_DOWN -> if (event.repeatCount == 0) {
                nativeOnGamepadInput(control, true, 1.0f)
            }
            KeyEvent.ACTION_UP -> nativeOnGamepadInput(control, false, 0.0f)
            else -> return super.dispatchKeyEvent(event)
        }
        return true
    }

    override fun dispatchGenericMotionEvent(event: MotionEvent): Boolean {
        if (event.source and InputDevice.SOURCE_JOYSTICK != InputDevice.SOURCE_JOYSTICK ||
            event.action != MotionEvent.ACTION_MOVE
        ) {
            return super.dispatchGenericMotionEvent(event)
        }
        val x = centeredAxis(event, MotionEvent.AXIS_X)
        val y = centeredAxis(event, MotionEvent.AXIS_Y)
        leftPressed = updateAxisButton("left", leftPressed, x <= if (leftPressed) -0.35f else -0.55f)
        rightPressed = updateAxisButton("right", rightPressed, x >= if (rightPressed) 0.35f else 0.55f)
        upPressed = updateAxisButton("up", upPressed, y <= if (upPressed) -0.35f else -0.55f)
        downPressed = updateAxisButton("down", downPressed, y >= if (downPressed) 0.35f else 0.55f)
        return true
    }

    private fun centeredAxis(event: MotionEvent, axis: Int): Float {
        val range = event.device?.getMotionRange(axis, event.source) ?: return 0.0f
        val value = event.getAxisValue(axis)
        return if (kotlin.math.abs(value) > range.flat) value.coerceIn(-1.0f, 1.0f) else 0.0f
    }

    private fun updateAxisButton(control: String, previous: Boolean, next: Boolean): Boolean {
        if (previous != next) {
            nativeOnGamepadInput(control, next, if (next) 1.0f else 0.0f)
        }
        return next
    }

    private fun requestGameAudioFocus() {
        val attributes = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_GAME)
            .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
            .build()
        audioFocusRequest = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
            .setAudioAttributes(attributes)
            .setOnAudioFocusChangeListener { focus -> nativeOnLifecycleChanged(if (focus > 0) 1 else 2) }
            .build()
        val result = audioManager.requestAudioFocus(audioFocusRequest!!)
        if (result != AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            nativeOnLifecycleChanged(2)
        }
    }

    private fun abandonGameAudioFocus() {
        audioFocusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
        audioFocusRequest = null
    }
}
