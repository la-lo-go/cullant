// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.importnotify

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationManagerCompat
import app.tauri.plugin.JSObject

/** Receives a tap on the Stop action of the progress notification. */
class ImportStopReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != ImportNotifier.ACTION_STOP) return
        val channel = ImportNotifier.stopChannel.takeIf { ImportNotifier.active }
        if (channel != null) channel.send(JSObject()) else NotificationManagerCompat.from(context).cancel(ImportNotifier.PROGRESS_ID)
    }
}
