// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.importnotify

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tauri.Logger
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class StartArgs {
    lateinit var projectName: String
}

@InvokeArg
class UpdateArgs {
    lateinit var phase: String
    var done: Long = 0
    var total: Long? = null
}

@InvokeArg
class FinishArgs {
    lateinit var outcome: String
    lateinit var projectName: String
    var fileCount: Long = 0
    var error: String? = null
}

private const val MIN_UPDATE_GAP_MS = 500L

@TauriPlugin(
    permissions = [Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "notifications")]
)
class ImportNotifyPlugin(private val activity: Activity) : Plugin(activity) {
    private val context get() = activity.applicationContext
    private var title = ""
    private var lastPhase = ""
    private var lastPostMs = 0L

    // Every call resolves, even on failure: a notification problem must never fail an import.
    private fun safely(invoke: Invoke, block: () -> Unit) {
        try {
            block()
        } catch (e: Exception) {
            Logger.error("ImportNotify", e.message ?: "notification call failed", e)
        }
        invoke.resolve(JSObject())
    }

    private fun notify(id: Int, notification: android.app.Notification) {
        try {
            NotificationManagerCompat.from(context).notify(id, notification)
        } catch (_: SecurityException) {
            // POST_NOTIFICATIONS denied on API 33+. The service still keeps the import alive.
        }
    }

    private fun stopService() {
        ImportNotifier.active = false
        context.stopService(Intent(context, ImportService::class.java))
    }

    @Command
    fun start(invoke: Invoke) {
        val args = invoke.parseArgs(StartArgs::class.java)
        safely(invoke) {
            title = args.projectName
            lastPhase = ""
            lastPostMs = 0L
            ImportNotifier.createChannels(context)
            ImportNotifier.active = true
            try {
                // Android 12+ refuses this from the background (ForegroundServiceStartNotAllowedException).
                ContextCompat.startForegroundService(
                    context,
                    Intent(context, ImportService::class.java).putExtra(ImportService.EXTRA_TITLE, title)
                )
            } catch (e: Exception) {
                ImportNotifier.active = false
                throw e
            }
        }
    }

    @Command
    fun update(invoke: Invoke) {
        val args = invoke.parseArgs(UpdateArgs::class.java)
        safely(invoke) {
            if (!ImportNotifier.active) return@safely
            val now = System.currentTimeMillis()
            val last = args.total != null && args.done >= args.total!!
            if (args.phase == lastPhase && !last && now - lastPostMs < MIN_UPDATE_GAP_MS) return@safely
            lastPhase = args.phase
            lastPostMs = now
            notify(
                ImportNotifier.PROGRESS_ID,
                ImportNotifier.progress(context, title, args.phase, args.done, args.total)
            )
        }
    }

    @Command
    fun finish(invoke: Invoke) {
        val args = invoke.parseArgs(FinishArgs::class.java)
        safely(invoke) {
            // Read before stopping: the service is what lowers the importance of a hidden app.
            val foreground = ImportNotifier.isForeground()
            stopService()
            if (!foreground) {
                notify(
                    ImportNotifier.RESULT_ID,
                    ImportNotifier.result(context, args.outcome, args.projectName, args.fileCount, args.error)
                )
            }
        }
    }

    @Command
    fun stop(invoke: Invoke) {
        safely(invoke) { stopService() }
    }

    @Command
    fun requestPermission(invoke: Invoke) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || notificationsGranted()) {
            invoke.resolve(JSObject().put("granted", true))
        } else {
            requestPermissionForAlias("notifications", invoke, "permissionResult")
        }
    }

    @PermissionCallback
    fun permissionResult(invoke: Invoke) {
        invoke.resolve(JSObject().put("granted", notificationsGranted()))
    }

    private fun notificationsGranted(): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED

    @Command
    fun openSettings(invoke: Invoke) {
        safely(invoke) {
            val intent = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                    .putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
            } else {
                Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                    .setData(Uri.fromParts("package", context.packageName, null))
            }
            activity.startActivity(intent)
        }
    }
}
