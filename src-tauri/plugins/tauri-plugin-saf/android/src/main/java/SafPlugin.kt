// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.saf

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.graphics.Matrix
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.ParcelFileDescriptor
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.util.Base64
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import app.tauri.Logger
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.ByteArrayOutputStream
import java.text.ParsePosition
import java.text.SimpleDateFormat
import java.util.Locale
import java.util.TimeZone
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit

@InvokeArg
class TreeArgs {
    lateinit var treeUri: String
}

@InvokeArg
class OpenTreeArgs {
    var preferRemovable: Boolean = false
}

@InvokeArg
class ListChildrenArgs {
    lateinit var treeUri: String
    lateinit var parentDocumentId: String
}

@InvokeArg
class FdArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
    lateinit var mode: String
}

@InvokeArg
class CreateDocumentArgs {
    lateinit var treeUri: String
    lateinit var parentDocumentId: String
    lateinit var mimeType: String
    lateinit var displayName: String
}

@InvokeArg
class RenameDocumentArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
    lateinit var newName: String
}

@InvokeArg
class MoveDocumentArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
    lateinit var sourceParentDocumentId: String
    lateinit var targetParentDocumentId: String
}

@InvokeArg
class CopyDocumentArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
    lateinit var targetParentDocumentId: String
}

@InvokeArg
class DocumentArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
}

@InvokeArg
class OpenDocumentArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
    var mimeType: String? = null
}

@InvokeArg
class VideoPosterArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
    // Longest edge the returned frame may have; 0 = native size.
    var maxEdge: Int = 0
}

@InvokeArg
class HeifStillArgs {
    lateinit var treeUri: String
    lateinit var documentId: String
    // Longest edge the returned image may have; 0 = native size.
    var maxEdge: Int = 0
}

@TauriPlugin
class SafPlugin(private val activity: Activity) : Plugin(activity) {
    private val resolver get() = activity.contentResolver
    private val workers = ThreadPoolExecutor(
        2, 2, 30L, TimeUnit.SECONDS, ArrayBlockingQueue<Runnable>(64)
    ).apply { allowCoreThreadTimeOut(true) }

    override fun onDestroy(activity: AppCompatActivity) {
        workers.shutdown()
    }

    private fun work(invoke: Invoke, task: () -> Unit) {
        try {
            workers.execute {
                try {
                    task()
                } catch (e: Exception) {
                    invoke.reject(e.message ?: "SAF operation failed")
                }
            }
        } catch (_: RejectedExecutionException) {
            invoke.reject("SAF worker queue is full or closed")
        }
    }

    private fun docUri(treeUri: String, documentId: String): Uri =
        DocumentsContract.buildDocumentUriUsingTree(Uri.parse(treeUri), documentId)

    @Command
    fun openTree(invoke: Invoke) {
        val args = invoke.parseArgs(OpenTreeArgs::class.java)
        val intent = preferredTreeIntent(args.preferRemovable)
        intent.addFlags(
            Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
        )
        startActivityForResult(invoke, intent, "openTreeResult")
    }

    private fun preferredTreeIntent(preferRemovable: Boolean): Intent {
        if (preferRemovable && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val sm = activity.getSystemService(Context.STORAGE_SERVICE) as StorageManager
            val mounted = sm.storageVolumes.filter {
                it.isRemovable &&
                    (it.state == Environment.MEDIA_MOUNTED ||
                        it.state == Environment.MEDIA_MOUNTED_READ_ONLY)
            }
            // Android exposes no stable UsbDevice -> StorageVolume mapping. If
            // there is one removable volume, its picker root is unambiguous.
            if (mounted.size == 1) return mounted.single().createOpenDocumentTreeIntent()
        }
        return Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
    }

    @Command
    fun consumeUsbAttach(invoke: Invoke) {
        val attached = activity.intent?.action == "android.hardware.usb.action.USB_DEVICE_ATTACHED"
        if (attached) activity.intent?.action = null
        val res = JSObject()
        res.put("value", attached)
        invoke.resolve(res)
    }

    @ActivityCallback
    fun openTreeResult(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_CANCELED) {
            val res = JSObject()
            res.put("cancelled", true)
            invoke.resolve(res)
            return
        }
        if (result.resultCode != Activity.RESULT_OK) {
            invoke.reject("folder picker failed with result ${result.resultCode}")
            return
        }
        val treeUri = result.data?.data
        if (treeUri == null) {
            invoke.reject("no tree uri returned")
            return
        }
        work(invoke) {
            try {
                val takeFlags =
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                val grantedFlags = (result.data?.flags ?: 0) and takeFlags
                require(grantedFlags == takeFlags) { "the folder needs read and write permission" }
                resolver.takePersistableUriPermission(treeUri, takeFlags)
                require(hasTreePermission(treeUri)) { "read and write permission was not persisted" }
                val rootId = DocumentsContract.getTreeDocumentId(treeUri)
                val res = JSObject()
                res.put("treeUri", treeUri.toString())
                res.put("rootDocumentId", rootId)
                invoke.resolve(res)
            } catch (e: Exception) {
                invoke.reject(e.message ?: "failed to persist tree permission")
            }
        }
    }

    @Command
    fun rootDocumentId(invoke: Invoke) {
        val args = invoke.parseArgs(TreeArgs::class.java)
        try {
            val uri = Uri.parse(args.treeUri)
            val rootId = if (DocumentsContract.isDocumentUri(activity, uri)) {
                DocumentsContract.getDocumentId(uri)
            } else {
                DocumentsContract.getTreeDocumentId(uri)
            }
            val res = JSObject()
            res.put("rootDocumentId", rootId)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to read root document id")
        }
    }

    @Command
    fun checkTreeAccess(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(TreeArgs::class.java)
        val uri = Uri.parse(args.treeUri)
        val ok = hasTreePermission(uri)
        val res = JSObject()
        res.put("ok", ok)
        invoke.resolve(res)
    }

    private fun hasTreePermission(uri: Uri): Boolean {
        val treeUri = DocumentsContract.buildTreeDocumentUri(
            uri.authority, DocumentsContract.getTreeDocumentId(uri)
        )
        return resolver.persistedUriPermissions.any {
            it.uri == treeUri && it.isReadPermission && it.isWritePermission
        }
    }

    // Report whether the tree URI's backing storage volume is currently present
    // (mounted), plus its user-visible name and removable flag, via
    // StorageManager. A volume that has been removed (ejected SD card / unplugged
    // USB) is absent from getStorageVolumes(), which we report as not mounted.
    @Command
    fun volumeInfo(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(TreeArgs::class.java)
        try {
            val treeUri = Uri.parse(args.treeUri)
            val treeDocId = DocumentsContract.getTreeDocumentId(treeUri)
            if (treeUri.authority != "com.android.externalstorage.documents") {
                val root = docUri(args.treeUri, treeDocId)
                val mounted = requireNotNull(resolver.query(
                    root, arrayOf(Document.COLUMN_DOCUMENT_ID), null, null, null
                )) { "provider returned no root cursor" }.use { it.moveToFirst() }
                val res = JSObject()
                res.put("mounted", mounted)
                res.put("removable", false)
                invoke.resolve(res)
                return@work
            }
            val volId = treeDocId.substringBefore(':')
            val sm = activity.getSystemService(Context.STORAGE_SERVICE) as StorageManager
            val match = sm.storageVolumes.firstOrNull { sv ->
                if (volId.equals("primary", ignoreCase = true)) sv.isPrimary
                else sv.uuid?.equals(volId, ignoreCase = true) == true
            }
            val res = JSObject()
            if (match != null) {
                res.put("mounted", match.state == Environment.MEDIA_MOUNTED ||
                    match.state == Environment.MEDIA_MOUNTED_READ_ONLY)
                res.put("removable", match.isRemovable)
                match.getDescription(activity)?.let { res.put("description", it) }
            } else {
                // Not in the volume list → physically absent. Only removable /
                // ejectable volumes disappear, so assume removable.
                res.put("mounted", false)
                res.put("removable", true)
            }
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to read volume info")
        }
    }

    @Command
    fun listChildren(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(ListChildrenArgs::class.java)
        val treeUri = Uri.parse(args.treeUri)
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(
            treeUri, args.parentDocumentId
        )
        val entries = JSArray()
        try {
            requireNotNull(resolver.query(
                childrenUri,
                arrayOf(
                    Document.COLUMN_DOCUMENT_ID,
                    Document.COLUMN_DISPLAY_NAME,
                    Document.COLUMN_MIME_TYPE,
                    Document.COLUMN_SIZE,
                    Document.COLUMN_LAST_MODIFIED
                ),
                null, null, null
            )) { "provider returned no child cursor" }.use { cursor ->
                val idIdx = cursor.getColumnIndexOrThrow(Document.COLUMN_DOCUMENT_ID)
                val nameIdx = cursor.getColumnIndexOrThrow(Document.COLUMN_DISPLAY_NAME)
                val mimeIdx = cursor.getColumnIndexOrThrow(Document.COLUMN_MIME_TYPE)
                val sizeIdx = cursor.getColumnIndexOrThrow(Document.COLUMN_SIZE)
                val mtimeIdx = cursor.getColumnIndexOrThrow(Document.COLUMN_LAST_MODIFIED)
                while (cursor.moveToNext()) {
                    val mime = cursor.getString(mimeIdx)
                    val entry = JSObject()
                    entry.put("documentId", cursor.getString(idIdx))
                    entry.put("name", cursor.getString(nameIdx) ?: "")
                    entry.put("isDir", mime == Document.MIME_TYPE_DIR)
                    entry.put("size", if (cursor.isNull(sizeIdx)) 0L else cursor.getLong(sizeIdx))
                    entry.put("mtime", if (cursor.isNull(mtimeIdx)) 0L else cursor.getLong(mtimeIdx))
                    entries.put(entry)
                }
            }
            val res = JSObject()
            res.put("entries", entries)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to list children")
        }
    }

    @Command
    fun getFileDescriptor(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(FdArgs::class.java)
        try {
            val fd = resolver.openFileDescriptor(docUri(args.treeUri, args.documentId), args.mode)
                ?.detachFd()
            val res = JSObject()
            res.put("fd", fd)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to open file descriptor")
        }
    }

    @Command
    fun createDocument(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(CreateDocumentArgs::class.java)
        try {
            val parent = docUri(args.treeUri, args.parentDocumentId)
            val created = DocumentsContract.createDocument(
                resolver, parent, args.mimeType, args.displayName
            ) ?: run {
                invoke.reject("createDocument returned null")
                return@work
            }
            resolveDocId(invoke, created)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to create document")
        }
    }

    @Command
    fun renameDocument(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(RenameDocumentArgs::class.java)
        try {
            val renamed = DocumentsContract.renameDocument(
                resolver, docUri(args.treeUri, args.documentId), args.newName
            ) ?: run {
                invoke.reject("renameDocument returned null")
                return@work
            }
            resolveDocId(invoke, renamed)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to rename document")
        }
    }

    @Command
    fun moveDocument(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(MoveDocumentArgs::class.java)
        try {
            val moved = DocumentsContract.moveDocument(
                resolver,
                docUri(args.treeUri, args.documentId),
                docUri(args.treeUri, args.sourceParentDocumentId),
                docUri(args.treeUri, args.targetParentDocumentId)
            ) ?: run {
                invoke.reject("moveDocument returned null")
                return@work
            }
            resolveDocId(invoke, moved)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to move document")
        }
    }

    @Command
    fun copyDocument(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(CopyDocumentArgs::class.java)
        try {
            val source = docUri(args.treeUri, args.documentId)
            val targetParent = docUri(args.treeUri, args.targetParentDocumentId)
            val copied = try {
                DocumentsContract.copyDocument(resolver, source, targetParent)
            } catch (_: Exception) {
                null
            }
            if (copied != null) {
                resolveDocId(invoke, copied)
                return@work
            }
            // Fallback: manual stream copy for providers without FLAG_SUPPORTS_COPY.
            resolveDocId(invoke, manualCopy(source, targetParent))
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to copy document")
        }
    }

    private fun manualCopy(source: Uri, targetParent: Uri): Uri {
        var name = "copy"
        var mime = "application/octet-stream"
        resolver.query(
            source,
            arrayOf(Document.COLUMN_DISPLAY_NAME, Document.COLUMN_MIME_TYPE),
            null, null, null
        )?.use { c ->
            if (c.moveToFirst()) {
                c.getString(0)?.let { name = it }
                c.getString(1)?.let { mime = it }
            }
        }
        val dest = DocumentsContract.createDocument(resolver, targetParent, mime, name)
            ?: throw IllegalStateException("manual copy: createDocument returned null")
        try {
            resolver.openInputStream(source).use { input ->
                resolver.openOutputStream(dest).use { output ->
                    requireNotNull(input) { "manual copy: cannot open source" }
                    requireNotNull(output) { "manual copy: cannot open dest" }
                    input.copyTo(output)
                }
            }
        } catch (copyError: Exception) {
            try {
                check(DocumentsContract.deleteDocument(resolver, dest)) {
                    "provider refused to remove incomplete copy $dest"
                }
            } catch (cleanupError: Exception) {
                throw IllegalStateException(
                    "copy failed: ${copyError.message}; incomplete document $dest remains: ${cleanupError.message}",
                    copyError
                )
            }
            throw copyError
        }
        return dest
    }

    @Command
    fun deleteDocument(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(DocumentArgs::class.java)
        try {
            val ok = DocumentsContract.deleteDocument(
                resolver, docUri(args.treeUri, args.documentId)
            )
            val res = JSObject()
            res.put("ok", ok)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to delete document")
        }
    }

    // Hand a document to an external app. The clip stays where it is (SAF content
    // URI); the launched app gets a temporary read grant for the lifetime of that
    // task. Used as the "open in an external player" fallback when the in-app
    // WebView can't decode a video.
    //
    // The bare ACTION_VIEW intent goes out first, on purpose. Android's own
    // resolver then handles it, which is what offers "Just once / Always" and
    // lets the user stop being asked — `Intent.createChooser` deliberately
    // suppresses that choice and always re-asks, which made picking a player a
    // permanent tax. The chooser survives only for the case it was really
    // covering: nothing on the device claims this document.
    @Command
    fun openDocument(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(OpenDocumentArgs::class.java)
        try {
            val uri = docUri(args.treeUri, args.documentId)
            val mime = args.mimeType ?: resolver.getType(uri) ?: "*/*"
            activity.runOnUiThread { launchDocument(invoke, uri, mime) }
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to read document type")
        }
    }

    private fun launchDocument(invoke: Invoke, uri: Uri, mime: String) {
        try {
            val view = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mime)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            try {
                activity.startActivity(view)
            } catch (_: ActivityNotFoundException) {
                activity.startActivity(
                    Intent.createChooser(view, null).apply {
                        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    }
                )
            }
            val res = JSObject()
            res.put("ok", true)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to open document")
        }
    }

    @Command
    fun videoMetadata(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(DocumentArgs::class.java)
        val uri = docUri(args.treeUri, args.documentId)
        val res = JSObject()
        runCatching { videoTrack(uri) }.getOrNull()?.let { putVideoTrack(res, it) }
        val retriever = MediaMetadataRetriever()
        try {
            retriever.setDataSource(activity, uri)
            putVideoMetadata(res, retriever)
        } catch (e: Exception) {
            Logger.warn("Video metadata is unavailable: ${e.message}")
        } finally {
            runCatching { retriever.release() }
        }
        invoke.resolve(res)
    }

    private fun videoTrack(uri: Uri): MediaFormat? {
        val extractor = MediaExtractor()
        try {
            extractor.setDataSource(activity, uri, null)
            return (0 until extractor.trackCount).asSequence().map { extractor.getTrackFormat(it) }
                .firstOrNull { it.getString(MediaFormat.KEY_MIME)?.startsWith("video/") == true }
        } finally {
            extractor.release()
        }
    }

    private fun putVideoTrack(res: JSObject, track: MediaFormat) {
        val mime = track.getString(MediaFormat.KEY_MIME) ?: return
        val codec = when (mime) {
            "video/avc" -> "h264"
            "video/hevc" -> "hevc"
            "video/av01" -> "av1"
            "video/x-vnd.on2.vp8" -> "vp8"
            "video/x-vnd.on2.vp9" -> "vp9"
            "video/mp4v-es" -> "mpeg4"
            "video/mpeg2" -> "mpeg2video"
            else -> mime.removePrefix("video/")
        }
        res.put("videoCodec", codec)
        val frameRate = runCatching { track.getInteger(MediaFormat.KEY_FRAME_RATE).toDouble() }
            .recoverCatching { track.getFloat(MediaFormat.KEY_FRAME_RATE).toDouble() }.getOrNull()
        frameRate?.takeIf { it.isFinite() && it > 0 }?.let { res.put("videoFrameRate", it) }
    }

    private fun putVideoMetadata(res: JSObject, retriever: MediaMetadataRetriever) {
        val width = intMeta(retriever, MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)
        val height = intMeta(retriever, MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)
        val rotation = intMeta(retriever, MediaMetadataRetriever.METADATA_KEY_VIDEO_ROTATION)
        if (width > 0 && height > 0) {
            res.put("width", width)
            res.put("height", height)
        }
        res.put("rotation", rotation)
        retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toDoubleOrNull()
            ?.takeIf { it.isFinite() && it > 0 }?.let { res.put("videoDuration", it / 1000) }
        if (!res.has("videoFrameRate")) {
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_CAPTURE_FRAMERATE)
                ?.toDoubleOrNull()?.takeIf { it.isFinite() && it > 0 }
                ?.let { res.put("videoFrameRate", it) }
        }
        recordedTime(retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DATE))
            ?.let { res.put("captureTime", it) }
    }

    private fun recordedTime(value: String?): Long? {
        if (value == null) return null
        for (pattern in arrayOf("yyyyMMdd'T'HHmmss.SSS'Z'", "yyyyMMdd'T'HHmmss'Z'")) {
            val parser = SimpleDateFormat(pattern, Locale.ROOT).apply {
                timeZone = TimeZone.getTimeZone("UTC")
                isLenient = false
            }
            val position = ParsePosition(0)
            val date = parser.parse(value, position)
            if (date != null && position.index == value.length && date.time > 0) return date.time / 1000
        }
        return null
    }

    // Extract one frame from a video as a JPEG. This is Cullant's only way to
    // thumbnail a video on Android: the desktop path shells out to ffmpeg, which
    // exists on no phone and could not read a content:// URI anyway. The frame
    // comes back base64-encoded because the Rust <-> Kotlin bridge carries JSON
    // and nothing else; a poster scaled to the grid's cell size is tens of
    // kilobytes, so the encoding overhead is irrelevant.
    //
    // `width`/`height` in the response are the clip's real display dimensions,
    // NOT the returned frame's — the caller records them as the video's size and
    // must not learn the thumbnail's size instead.
    @Command
    fun videoPoster(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(VideoPosterArgs::class.java)
        val retriever = MediaMetadataRetriever()
        try {
            retriever.setDataSource(activity, docUri(args.treeUri, args.documentId))

            val stored = intMeta(retriever, MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH) to
                intMeta(retriever, MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)
            val rotation = intMeta(retriever, MediaMetadataRetriever.METADATA_KEY_VIDEO_ROTATION)
            // The width/height keys report the dimensions as stored, without
            // folding in the rotation (AOSP records the angle as a separate key
            // and never swaps them), so a quarter turn swaps them here into what
            // the viewer is meant to see.
            val display =
                if (rotation == 90 || rotation == 270) stored.second to stored.first else stored

            val frame = firstFrame(retriever, args.maxEdge) ?: run {
                invoke.reject("no frame could be extracted")
                return@work
            }
            val oriented = orient(frame, rotation, display)
            val scaled = clampToMaxEdge(oriented, args.maxEdge)

            val out = ByteArrayOutputStream()
            scaled.compress(Bitmap.CompressFormat.JPEG, POSTER_QUALITY, out)

            val res = JSObject()
            res.put("jpegBase64", Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP))
            // Fall back to the frame's own size for a clip whose metadata does not
            // report its dimensions, rather than claiming a size of zero.
            res.put("width", if (display.first > 0) display.first else scaled.width)
            res.put("height", if (display.second > 0) display.second else scaled.height)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to extract a video poster")
        } finally {
            try {
                retriever.release()
            } catch (_: Exception) {
                // A retriever that cannot be released is already unusable.
            }
        }
    }

    // Whether this device can decode HEIF at all. ImageDecoder reads it from
    // API 28; the app supports 24, so the Rust side must ask rather than assume.
    @Command
    fun heifSupported(invoke: Invoke) {
        val res = JSObject()
        res.put("supported", Build.VERSION.SDK_INT >= Build.VERSION_CODES.P)
        invoke.resolve(res)
    }

    // Decode a HEIF still through the platform, because Cullant has no HEVC
    // decoder of its own and a phone has no ffmpeg binary to borrow one from.
    // The image comes back base64-encoded for the same reason a video poster
    // does: the Rust <-> Kotlin bridge carries JSON and nothing else.
    //
    // `width`/`height` in the response are the file's REAL dimensions, not the
    // returned image's — the caller records them as the photo's size, and a
    // sampled decode would otherwise teach it the thumbnail's size instead.
    @Command
    fun heifStill(invoke: Invoke) = work(invoke) {
        val args = invoke.parseArgs(HeifStillArgs::class.java)
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            invoke.reject("this device has no HEIF decoder")
            return@work
        }
        try {
            val uri = docUri(args.treeUri, args.documentId)
            var original = 0 to 0
            val source = ImageDecoder.createSource(resolver, uri)
            val bitmap = ImageDecoder.decodeBitmap(source) { decoder, info, _ ->
                original = info.size.width to info.size.height
                // Sample down during the decode. A 48 MP HEIC is ~190 MB of
                // ARGB, and this process has no largeHeap — decoding it whole to
                // throw all but a grid cell away is how the app gets killed.
                val cap = if (args.maxEdge > 0) args.maxEdge else MAX_STILL_EDGE
                val longEdge = maxOf(info.size.width, info.size.height)
                if (longEdge > cap) {
                    val scale = cap.toDouble() / longEdge
                    decoder.setTargetSize(
                        maxOf(1, Math.round(info.size.width * scale).toInt()),
                        maxOf(1, Math.round(info.size.height * scale).toInt())
                    )
                }
                decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            }

            val out = ByteArrayOutputStream()
            bitmap.compress(Bitmap.CompressFormat.JPEG, POSTER_QUALITY, out)

            val res = JSObject()
            res.put("jpegBase64", Base64.encodeToString(out.toByteArray(), Base64.NO_WRAP))
            res.put("width", if (original.first > 0) original.first else bitmap.width)
            res.put("height", if (original.second > 0) original.second else bitmap.height)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to decode a HEIF still")
        }
    }

    // A frame from a second in, falling back to the very start for clips shorter
    // than that. Mirrors the desktop ffmpeg path: a small skip past the start
    // avoids the black leader frames many clips open on.
    private fun firstFrame(retriever: MediaMetadataRetriever, maxEdge: Int): Bitmap? {
        for (timeUs in longArrayOf(1_000_000L, 0L)) {
            // A square target box: getScaledFrameAtTime preserves the aspect
            // ratio and fits the frame inside it, so this caps the long edge
            // whichever way round the clip is — no rotation guesswork needed.
            // Scaling during extraction beats decoding a 4K frame to throw all
            // but a grid cell of it away, but it is the newer and less
            // universally implemented call, so a device that refuses it drops to
            // the plain one rather than losing the poster.
            if (maxEdge > 0 && Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
                try {
                    retriever.getScaledFrameAtTime(
                        timeUs, MediaMetadataRetriever.OPTION_CLOSEST_SYNC, maxEdge, maxEdge
                    )?.let { return it }
                } catch (e: Exception) {
                    Logger.warn("getScaledFrameAtTime failed, falling back: ${e.message}")
                }
            }
            retriever.getFrameAtTime(timeUs, MediaMetadataRetriever.OPTION_CLOSEST_SYNC)
                ?.let { return it }
        }
        return null
    }

    // Rotate a frame that came back in the clip's stored orientation.
    //
    // Recent Android releases already apply the rotation metadata to the frame
    // they hand back, but older ones do not, and the platform documents neither
    // behaviour. So we detect it instead of assuming: a frame still in stored
    // orientation is the one whose portrait/landscape sense disagrees with the
    // display dimensions.
    //
    // That test cannot see a half turn, which leaves the aspect ratio alone — so
    // a 180-rotated clip on a release that does not rotate for us comes out
    // upside down. Only a phone held inverted records one, and any release that
    // applies a quarter turn applies a half turn too, so the case is doubly rare.
    private fun orient(frame: Bitmap, rotation: Int, display: Pair<Int, Int>): Bitmap {
        val (dw, dh) = display
        if (rotation == 0 || dw <= 0 || dh <= 0 || dw == dh) return frame
        if ((frame.width > frame.height) == (dw > dh)) return frame
        val matrix = Matrix().apply { postRotate(rotation.toFloat()) }
        return Bitmap.createBitmap(frame, 0, 0, frame.width, frame.height, matrix, true)
    }

    // Safety net for the getFrameAtTime fallback, which ignores the target size.
    private fun clampToMaxEdge(frame: Bitmap, maxEdge: Int): Bitmap {
        val longEdge = maxOf(frame.width, frame.height)
        if (maxEdge <= 0 || longEdge <= maxEdge) return frame
        val scale = maxEdge.toDouble() / longEdge
        val w = maxOf(1, Math.round(frame.width * scale).toInt())
        val h = maxOf(1, Math.round(frame.height * scale).toInt())
        return Bitmap.createScaledBitmap(frame, w, h, true)
    }

    private fun intMeta(retriever: MediaMetadataRetriever, key: Int): Int =
        retriever.extractMetadata(key)?.toIntOrNull() ?: 0

    private fun resolveDocId(invoke: Invoke, uri: Uri) {
        val res = JSObject()
        res.put("documentId", DocumentsContract.getDocumentId(uri))
        invoke.resolve(res)
    }

    private companion object {
        // The frame is re-encoded downstream at the cache's own quality, so this
        // only has to survive one round trip without visible loss.
        const val POSTER_QUALITY = 90

        // Hard ceiling on a decoded still, whatever the caller asks for. The
        // focus check asks for native size, and a 48 MP HEIC at native size is
        // ~190 MB of ARGB in a process with no largeHeap.
        const val MAX_STILL_EDGE = 4096
    }
}
