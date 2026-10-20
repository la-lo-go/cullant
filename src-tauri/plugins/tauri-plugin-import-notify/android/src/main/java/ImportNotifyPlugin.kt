// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.importnotify

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.webkit.WebView
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

@TauriPlugin(
    permissions = [Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "notifications")]
)
class ImportNotifyPlugin(private val activity: Activity) : Plugin(activity) {
    private val context get() = activity.applicationContext
    private var title = ""

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

    @Command
    fun start(invoke: Invoke) {
        val args = invoke.parseArgs(StartArgs::class.java)
        safely(invoke) {
            title = args.projectName
            ImportNotifier.createChannels(context)
            NotificationManagerCompat.from(context).cancel(ImportNotifier.RESULT_ID)
            // Android 12+ refuses to start a foreground service from the background, and the
            // app then crashes if the service never enters the foreground. Do not try.
            if (!ImportNotifier.isForeground()) {
                Logger.warn("ImportNotify", "App is in the background, not starting the service")
                ImportNotifier.deferStart()
                return@safely
            }
            ImportNotifier.requestStart()
            startService()
        }
    }

    private fun startService() {
        try {
            ContextCompat.startForegroundService(
                context,
                Intent(context, ImportService::class.java).putExtra(ImportService.EXTRA_TITLE, title)
            )
        } catch (e: Exception) {
            ImportNotifier.serviceEnded()
            throw e
        }
    }

    // A process killed mid-import leaves its ongoing notification behind. A new
    // process runs no import yet, so nobody else owns the progress id.
    override fun load(webView: WebView) {
        try {
            if (!ImportNotifier.active) NotificationManagerCompat.from(context).cancel(ImportNotifier.PROGRESS_ID)
        } catch (e: Exception) {
            Logger.error("ImportNotify", e.message ?: "cleanup failed", e)
        }
    }

    // The app is visible again: a refused start can go ahead now, and a result is no news.
    override fun onResume() {
        try {
            NotificationManagerCompat.from(context).cancel(ImportNotifier.RESULT_ID)
            if (ImportNotifier.resumePending()) startService()
        } catch (e: Exception) {
            Logger.error("ImportNotify", e.message ?: "resume failed", e)
        }
    }

    @Command
    fun update(invoke: Invoke) {
        val args = invoke.parseArgs(UpdateArgs::class.java)
        safely(invoke) {
            if (!ImportNotifier.active) return@safely
            val notification = ImportNotifier.progress(context, title, args.phase, args.done, args.total)
            ImportNotifier.latest = notification
            notify(ImportNotifier.PROGRESS_ID, notification)
        }
    }

    @Command
    fun finish(invoke: Invoke) {
        val args = invoke.parseArgs(FinishArgs::class.java)
        safely(invoke) {
            // Read before stopping, so the answer does not depend on how fast Android re-ranks the process.
            val foreground = ImportNotifier.isForeground()
            ImportNotifier.requestStop(context)
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
        safely(invoke) { ImportNotifier.requestStop(context) }
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
