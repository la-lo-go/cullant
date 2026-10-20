// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.importnotify

import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import android.util.Log
import androidx.core.app.ServiceCompat

/** Holds the process alive while an import runs. The plugin posts the progress updates. */
class ImportService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val title = intent?.getStringExtra(EXTRA_TITLE) ?: ""
        try {
            ImportNotifier.createChannels(this)
            ServiceCompat.startForeground(
                this,
                ImportNotifier.PROGRESS_ID,
                ImportNotifier.progress(this, title, "preparing", 0, null),
                ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
            )
        } catch (e: Exception) {
            Log.w(TAG, "Cannot start the foreground service", e)
            ImportNotifier.active = false
            stopSelf()
        }
        return START_NOT_STICKY
    }

    // Android 15 ends a dataSync service after about six hours and requires it to stop.
    override fun onTimeout(startId: Int, fgsType: Int) {
        ImportNotifier.active = false
        stopSelf()
    }

    override fun onDestroy() {
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    companion object {
        const val EXTRA_TITLE = "title"
        private const val TAG = "ImportService"
    }
}
