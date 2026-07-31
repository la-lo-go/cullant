// Storage Access Framework bridge for Cullant.
// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.saf

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Matrix
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Build
import android.os.ParcelFileDescriptor
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import android.util.Base64
import androidx.activity.result.ActivityResult
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

@InvokeArg
class TreeArgs {
    lateinit var treeUri: String
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
class DeleteDocumentArgs {
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

@TauriPlugin
class SafPlugin(private val activity: Activity) : Plugin(activity) {
    private val resolver get() = activity.contentResolver

    private fun docUri(treeUri: String, documentId: String): Uri =
        DocumentsContract.buildDocumentUriUsingTree(Uri.parse(treeUri), documentId)

    // ---- folder picker ----

    @Command
    fun openTree(invoke: Invoke) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
        intent.addFlags(
            Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
        )
        startActivityForResult(invoke, intent, "openTreeResult")
    }

    @ActivityCallback
    fun openTreeResult(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode != Activity.RESULT_OK) {
            invoke.reject("cancelled")
            return
        }
        val treeUri = result.data?.data
        if (treeUri == null) {
            invoke.reject("no tree uri returned")
            return
        }
        try {
            val takeFlags =
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            resolver.takePersistableUriPermission(treeUri, takeFlags)
            val rootId = DocumentsContract.getTreeDocumentId(treeUri)
            val res = JSObject()
            res.put("treeUri", treeUri.toString())
            res.put("rootDocumentId", rootId)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to persist tree permission")
        }
    }

    @Command
    fun rootDocumentId(invoke: Invoke) {
        val args = invoke.parseArgs(TreeArgs::class.java)
        try {
            val rootId = DocumentsContract.getTreeDocumentId(Uri.parse(args.treeUri))
            val res = JSObject()
            res.put("rootDocumentId", rootId)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to read root document id")
        }
    }

    @Command
    fun checkTreeAccess(invoke: Invoke) {
        val args = invoke.parseArgs(TreeArgs::class.java)
        val uri = Uri.parse(args.treeUri)
        val ok = resolver.persistedUriPermissions.any {
            it.uri == uri && it.isReadPermission
        }
        val res = JSObject()
        res.put("ok", ok)
        invoke.resolve(res)
    }

    // Report whether the tree URI's backing storage volume is currently present
    // (mounted), plus its user-visible name and removable flag, via
    // StorageManager. A volume that has been removed (ejected SD card / unplugged
    // USB) is absent from getStorageVolumes(), which we report as not mounted.
    @Command
    fun volumeInfo(invoke: Invoke) {
        val args = invoke.parseArgs(TreeArgs::class.java)
        try {
            val treeDocId = DocumentsContract.getTreeDocumentId(Uri.parse(args.treeUri))
            val volId = treeDocId.substringBefore(':')
            val sm = activity.getSystemService(Context.STORAGE_SERVICE) as StorageManager
            val match = sm.storageVolumes.firstOrNull { sv ->
                if (volId.equals("primary", ignoreCase = true)) sv.isPrimary
                else sv.uuid?.equals(volId, ignoreCase = true) == true
            }
            val res = JSObject()
            if (match != null) {
                res.put("mounted", true)
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

    // ---- listing ----

    @Command
    fun listChildren(invoke: Invoke) {
        val args = invoke.parseArgs(ListChildrenArgs::class.java)
        val treeUri = Uri.parse(args.treeUri)
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(
            treeUri, args.parentDocumentId
        )
        val entries = JSArray()
        try {
            resolver.query(
                childrenUri,
                arrayOf(
                    Document.COLUMN_DOCUMENT_ID,
                    Document.COLUMN_DISPLAY_NAME,
                    Document.COLUMN_MIME_TYPE,
                    Document.COLUMN_SIZE,
                    Document.COLUMN_LAST_MODIFIED
                ),
                null, null, null
            )?.use { cursor ->
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

    // ---- file descriptor bridge ----

    @Command
    fun getFileDescriptor(invoke: Invoke) {
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

    // ---- mutations ----

    @Command
    fun createDocument(invoke: Invoke) {
        val args = invoke.parseArgs(CreateDocumentArgs::class.java)
        try {
            val parent = docUri(args.treeUri, args.parentDocumentId)
            val created = DocumentsContract.createDocument(
                resolver, parent, args.mimeType, args.displayName
            ) ?: run {
                invoke.reject("createDocument returned null")
                return
            }
            resolveDocId(invoke, created)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to create document")
        }
    }

    @Command
    fun renameDocument(invoke: Invoke) {
        val args = invoke.parseArgs(RenameDocumentArgs::class.java)
        try {
            val renamed = DocumentsContract.renameDocument(
                resolver, docUri(args.treeUri, args.documentId), args.newName
            ) ?: run {
                invoke.reject("renameDocument returned null")
                return
            }
            resolveDocId(invoke, renamed)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to rename document")
        }
    }

    @Command
    fun moveDocument(invoke: Invoke) {
        val args = invoke.parseArgs(MoveDocumentArgs::class.java)
        try {
            val moved = DocumentsContract.moveDocument(
                resolver,
                docUri(args.treeUri, args.documentId),
                docUri(args.treeUri, args.sourceParentDocumentId),
                docUri(args.treeUri, args.targetParentDocumentId)
            ) ?: run {
                invoke.reject("moveDocument returned null")
                return
            }
            resolveDocId(invoke, moved)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to move document")
        }
    }

    @Command
    fun copyDocument(invoke: Invoke) {
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
                return
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
        resolver.openInputStream(source).use { input ->
            resolver.openOutputStream(dest).use { output ->
                requireNotNull(input) { "manual copy: cannot open source" }
                requireNotNull(output) { "manual copy: cannot open dest" }
                input.copyTo(output)
            }
        }
        return dest
    }

    @Command
    fun deleteDocument(invoke: Invoke) {
        val args = invoke.parseArgs(DeleteDocumentArgs::class.java)
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
    fun openDocument(invoke: Invoke) {
        val args = invoke.parseArgs(OpenDocumentArgs::class.java)
        try {
            val uri = docUri(args.treeUri, args.documentId)
            val mime = args.mimeType ?: resolver.getType(uri) ?: "*/*"
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

    // ---- video poster frames ----

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
    fun videoPoster(invoke: Invoke) {
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
                return
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
    }
}
