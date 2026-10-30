package com.rosmontis220.cfmsmodded

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {

    companion object {
        /** Notification channel ID used by the foreground service. */
        const val NOTIFICATION_CHANNEL_ID = "cfms_background_service"
        /** Human-readable channel name shown in system settings. */
        const val NOTIFICATION_CHANNEL_NAME = "CFMS Background Service"
    }

    /** Launcher for the POST_NOTIFICATIONS runtime permission on Android 13+. */
    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { /* granted or denied */ }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        // Create the notification channel required for the foreground service.
        // This must happen before the service starts.
        createNotificationChannel()

        // Request POST_NOTIFICATIONS on Android 13+ (API 33).
        // The foreground service requires a visible notification; without
        // permission the notification is silently hidden.
        requestNotificationPermission()
    }

    /**
     * Keeps the page above the on-screen keyboard.
     *
     * `enableEdgeToEdge()` stops the window from resizing when the IME opens, so
     * the document keeps the full screen height and composers pinned to the
     * bottom of a full-height column end up underneath the keyboard. The window
     * insets still report the IME, so the WebView is shrunk by that amount.
     */
    override fun onWebViewCreate(webView: WebView) {
        super.onWebViewCreate(webView)

        ViewCompat.setOnApplyWindowInsetsListener(webView) { view, insets ->
            resizeAboveKeyboard(view, insets.getInsets(WindowInsetsCompat.Type.ime()).bottom)
            insets
        }
        ViewCompat.requestApplyInsets(webView)
    }

    /**
     * Shrinks the WebView by the height of the on-screen keyboard.
     *
     * Shrinking the view is what makes the page notice: Chromium relays the new
     * size to the document, so `window.innerHeight`, `100%` heights and
     * `visualViewport` all follow the visible area again. Pushing the inset into
     * the page with JavaScript cannot work here because the document has no way
     * to grow the window back when the keyboard closes.
     */
    private fun resizeAboveKeyboard(view: View, imeBottom: Int) {
        val layoutParams = view.layoutParams ?: return
        val target = if (imeBottom > 0) {
            val host = (view.parent as? View)?.height?.takeIf { it > 0 } ?: view.rootView.height
            (host - imeBottom).coerceAtLeast(1)
        } else {
            ViewGroup.LayoutParams.MATCH_PARENT
        }
        if (layoutParams.height != target) {
            layoutParams.height = target
            view.layoutParams = layoutParams
        }
    }

    /**
     * Creates the mandatory notification channel for Android 8.0+ (API 26).
     * No-op on older versions.
     */
    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                NOTIFICATION_CHANNEL_ID,
                NOTIFICATION_CHANNEL_NAME,
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Persistent notification for CFMS background services " +
                    "(token refresh, file sync, lockdown monitoring)"
                setShowBadge(false)
            }
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(channel)
        }
    }

    /**
     * Requests the POST_NOTIFICATIONS runtime permission on Android 13+.
     * The foreground service cannot display its mandatory notification without
     * this permission.
     */
    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            if (ContextCompat.checkSelfPermission(
                    this, Manifest.permission.POST_NOTIFICATIONS
                ) != PackageManager.PERMISSION_GRANTED
            ) {
                notificationPermissionLauncher.launch(
                    Manifest.permission.POST_NOTIFICATIONS
                )
            }
        }
    }
}
