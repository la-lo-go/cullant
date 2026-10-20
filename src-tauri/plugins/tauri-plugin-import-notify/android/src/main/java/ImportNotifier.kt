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
import app.tauri.plugin.Channel
import java.text.NumberFormat

/** Channels, notification builders and the shared "is an import running" flag. */
object ImportNotifier {
    const val PROGRESS_CHANNEL = "import_progress"
    const val DONE_CHANNEL = "import_done"
    const val ERROR_CHANNEL = "import_error"
    const val PROGRESS_ID = 1
    const val RESULT_ID = 2
    const val ACTION_STOP = "app.tauri.importnotify.STOP"

    // A killed process cannot cancel its own notification, so progress expires by itself.
    // The Rust worker repeats the latest update every minute while an import runs.
    private const val PROGRESS_TIMEOUT_MS = 5 * 60 * 1000L

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

    // Rust listens here for a tap on Stop. It is process-wide, not owned by a plugin
    // instance, so it survives a recreated activity. A restarted process has none, and
    // the tap only removes the orphaned notification.
    @Volatile
    var stopChannel: Channel? = null

    // An import whose service start was refused because the app was hidden.
    // The next resume starts the service, unless the import ended in between.
    private var startDeferred = false

    @Synchronized
    fun requestStart() {
        active = true
        latest = null
        serviceRequested = true
        foregroundReached = false
        stopRequested = false
        startDeferred = false
    }

    @Synchronized
    fun deferStart() {
        startDeferred = true
    }

    /** Takes the deferred start and requests it in one step, so a stop cannot slip in between. */
    @Synchronized
    fun resumePending(): Boolean {
        if (!startDeferred) return false
        requestStart()
        return true
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
        startDeferred = false
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

    private fun stopIntent(context: Context): PendingIntent = PendingIntent.getBroadcast(
        context,
        1,
        Intent(ACTION_STOP).setClass(context, ImportStopReceiver::class.java),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun base(context: Context, channel: String, title: String): NotificationCompat.Builder =
        NotificationCompat.Builder(context, channel)
            .setSmallIcon(R.drawable.ic_import_notify)
            .setContentTitle(title)
            .setContentIntent(contentIntent(context))

    private fun timeLeft(minutes: Int): String = when {
        minutes < 1 -> "less than a minute left"
        minutes < 60 -> "about $minutes min left"
        else -> "about ${minutes / 60} h left"
    }

    // The title counts the current phase. The bar and the time left cover the whole import.
    fun progress(
        context: Context,
        title: String,
        phase: String,
        done: Long,
        total: Long?,
        permille: Int?,
        etaMinutes: Int?,
    ): Notification {
        val count = when {
            phase !in PHASES -> null
            total == null -> "${format(done)} found"
            else -> "${format(done)} / ${format(total)}"
        }
        val text = listOfNotNull(label(phase), etaMinutes?.let(::timeLeft)).joinToString(" · ")
        val builder = base(context, PROGRESS_CHANNEL, if (count == null) title else "$title · $count")
            .setContentText(text)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setTimeoutAfter(PROGRESS_TIMEOUT_MS)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            // Android 12+ delays a foreground service notification by up to 10 s otherwise.
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .addAction(0, "Stop", stopIntent(context))
        if (permille == null) {
            builder.setProgress(0, 0, true)
        } else {
            builder.setProgress(1000, permille.coerceIn(0, 1000), false)
        }
        return builder.build()
    }

    private fun counts(args: FinishArgs): String = listOfNotNull(
        args.photos.takeIf { it > 0 }?.let { "${format(it)} ${if (it == 1L) "photo" else "photos"}" },
        args.videos.takeIf { it > 0 }?.let { "${format(it)} ${if (it == 1L) "video" else "videos"}" },
    ).joinToString(" · ")

    fun result(context: Context, args: FinishArgs): Notification {
        val name = args.projectName
        val (channel, title, text) = when (args.outcome) {
            "done" -> Triple(DONE_CHANNEL, "Project ready", listOf(name, counts(args)).filter { it.isNotEmpty() }.joinToString(" · "))
            "empty" -> Triple(DONE_CHANNEL, "No photos or videos found", name)
            else -> Triple(ERROR_CHANNEL, "Import failed", listOfNotNull(name, args.error?.lineSequence()?.first()?.take(MAX_ERROR_CHARS)?.ifBlank { null }).joinToString(" · "))
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
