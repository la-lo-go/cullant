// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.importnotify

import android.app.ActivityManager
import android.app.Notification
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationChannelCompat
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import java.text.NumberFormat

/** Channels, notification builders and the shared "is an import running" flag. */
object ImportNotifier {
    const val PROGRESS_CHANNEL = "import_progress"
    const val DONE_CHANNEL = "import_done"
    const val ERROR_CHANNEL = "import_error"
    const val PROGRESS_ID = 1
    const val RESULT_ID = 2

    // The service can end on its own (timeout), so the plugin must not keep
    // posting progress under an id that no service owns any more.
    @Volatile
    var active = false

    // Latest progress notification. The service shows it when it enters the foreground.
    @Volatile
    var latest: Notification? = null

    // Android crashes the app if a started foreground service stops before it called
    // startForeground. A stop that arrives early is therefore deferred to the service.
    private var serviceRequested = false
    private var foregroundReached = false
    private var stopRequested = false

    // Title of an import whose service start was refused because the app was hidden.
    // The next resume starts the service, unless the import ended in between.
    private var pendingTitle: String? = null

    @Synchronized
    fun requestStart() {
        active = true
        latest = null
        serviceRequested = true
        foregroundReached = false
        stopRequested = false
        pendingTitle = null
    }

    @Synchronized
    fun deferStart(title: String) {
        pendingTitle = title
    }

    /** Takes the deferred start and requests it in one step, so a stop cannot slip in between. */
    @Synchronized
    fun resumePending(): String? {
        val title = pendingTitle ?: return null
        requestStart()
        return title
    }

    /** Called by the service after startForeground. True when a stop is waiting. */
    @Synchronized
    fun reachedForeground(): Boolean {
        foregroundReached = true
        if (stopRequested) serviceRequested = false
        return stopRequested
    }

    @Synchronized
    fun serviceEnded() {
        active = false
        serviceRequested = false
    }

    @Synchronized
    fun requestStop(context: Context) {
        active = false
        pendingTitle = null
        if (!serviceRequested) return
        if (foregroundReached) {
            serviceRequested = false
            context.stopService(Intent(context, ImportService::class.java))
        } else {
            stopRequested = true
        }
    }

    fun createChannels(context: Context) {
        val manager = NotificationManagerCompat.from(context)
        manager.createNotificationChannel(
            NotificationChannelCompat.Builder(PROGRESS_CHANNEL, NotificationManagerCompat.IMPORTANCE_LOW)
                .setName("Import progress")
                .setDescription("Shows progress while a project imports.")
                .setSound(null, null)
                .build()
        )
        manager.createNotificationChannel(
            NotificationChannelCompat.Builder(DONE_CHANNEL, NotificationManagerCompat.IMPORTANCE_DEFAULT)
                .setName("Import finished")
                .setDescription("Tells you when a project is ready.")
                .build()
        )
        manager.createNotificationChannel(
            NotificationChannelCompat.Builder(ERROR_CHANNEL, NotificationManagerCompat.IMPORTANCE_HIGH)
                .setName("Import errors")
                .setDescription("Tells you when a project import fails.")
                .build()
        )
    }

    private fun format(n: Long): String = NumberFormat.getIntegerInstance().format(n)

    private fun label(phase: String): String = when (phase) {
        "finding" -> "Finding files"
        "metadata" -> "Reading metadata"
        "first_thumbnails" -> "Creating first thumbnails"
        "thumbnails" -> "Creating thumbnails"
        "previews" -> "Creating previews"
        "videos" -> "Creating video posters"
        else -> "Preparing import"
    }

    private fun contentIntent(context: Context): PendingIntent? {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return null
        return PendingIntent.getActivity(
            context, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
    }

    private fun base(context: Context, channel: String, title: String): NotificationCompat.Builder =
        NotificationCompat.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_import_notify)
            .setContentTitle(title)
            .setContentIntent(contentIntent(context))

    fun progress(context: Context, title: String, phase: String, done: Long, total: Long?): Notification {
        val count = when {
            phase !in PHASES -> null
            total == null -> "${format(done)} found"
            else -> "${format(done)} / ${format(total)}"
        }
        val builder = base(context, PROGRESS_CHANNEL, if (count == null) title else "$title · $count")
            .setContentText(label(phase))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
        if (total == null || total <= 0) {
            builder.setProgress(0, 0, true)
        } else {
            // Progress takes Int; scale so a very large library cannot overflow.
            val max = 1000
            builder.setProgress(max, (done.coerceIn(0, total) * max / total).toInt(), false)
        }
        return builder.build()
    }

    fun result(context: Context, outcome: String, name: String, fileCount: Long, error: String?): Notification {
        val (channel, title, text) = when (outcome) {
            "done" -> Triple(DONE_CHANNEL, "Project ready", "$name · ${format(fileCount)} files")
            "empty" -> Triple(DONE_CHANNEL, "No photos or videos found", name)
            else -> Triple(ERROR_CHANNEL, "Import failed", listOfNotNull(name, error?.lineSequence()?.first()?.take(MAX_ERROR_CHARS)?.ifBlank { null }).joinToString(" · "))
        }
        return base(context, channel, title)
            .setContentText(text)
            .setAutoCancel(true)
            .build()
    }

    /** True when the app has a visible top activity. A hidden app that runs a foreground service ranks lower. */
    fun isForeground(): Boolean {
        val info = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(info)
        // VISIBLE counts: an activity under a permission dialog may still start a foreground service.
        // FOREGROUND_SERVICE (125) sits between the two and must not count.
        return info.importance == ActivityManager.RunningAppProcessInfo.IMPORTANCE_FOREGROUND ||
            info.importance == ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE
    }

    private const val MAX_ERROR_CHARS = 120
    private val PHASES = setOf("finding", "metadata", "first_thumbnails", "thumbnails", "previews", "videos")
}
