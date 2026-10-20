// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.importnotify

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.core.app.ServiceCompat

/** Holds the process alive while an import runs. The plugin posts the progress updates. */
class ImportService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val title = intent?.getStringExtra(EXTRA_TITLE) ?: ""
        ImportNotifier.createChannels(this)
        // Progress may have been posted before this runs. Do not overwrite it.
        val notification = ImportNotifier.latest
            ?: ImportNotifier.progress(this, title, "preparing", 0, null)
        // The fallback in enterForeground cannot cover a startForeground failure thrown before Android clears fgRequired.
        if (!enterForeground(notification)) {
            ImportNotifier.serviceEnded()
            stopSelf()
            return START_NOT_STICKY
        }
        holdWakeLock()
        // A stop that arrived before this point could not stop the service safely:
        // Android crashes the app when a service stops without reaching startForeground.
        if (ImportNotifier.reachedForeground()) {
            stopSelf()
        }
        return START_NOT_STICKY
    }

    // A foreground service does not keep the CPU awake with the screen off.
    // The timeout is a safety net that matches the Android 15 dataSync budget.
    private fun holdWakeLock() {
        if (wakeLock?.isHeld == true) return
        val manager = getSystemService(POWER_SERVICE) as PowerManager
        wakeLock = manager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "cullant:import")
            .apply { acquire(WAKE_LOCK_TIMEOUT_MS) }
    }

    private fun releaseWakeLock() {
        wakeLock?.takeIf { it.isHeld }?.release()
        wakeLock = null
    }

    // Second try without the type drops the one thing a stricter Android version may refuse.
    private fun enterForeground(notification: android.app.Notification): Boolean {
        val id = ImportNotifier.PROGRESS_ID
        return try {
            ServiceCompat.startForeground(this, id, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            true
        } catch (e: Exception) {
            Log.w(TAG, "startForeground with a type failed", e)
            try {
                startForeground(id, notification)
                true
            } catch (e2: Exception) {
                Log.w(TAG, "startForeground failed", e2)
                false
            }
        }
    }

    // Android 15 ends a dataSync service after about six hours and requires it to stop.
    override fun onTimeout(startId: Int, fgsType: Int) {
        releaseWakeLock()
        ImportNotifier.serviceEnded()
        stopSelf()
    }

    override fun onDestroy() {
        releaseWakeLock()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    companion object {
        const val EXTRA_TITLE = "title"
        private const val TAG = "ImportService"
        private const val WAKE_LOCK_TIMEOUT_MS = 6 * 60 * 60 * 1000L
    }
}
