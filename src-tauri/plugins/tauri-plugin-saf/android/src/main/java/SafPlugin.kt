// Storage Access Framework bridge for Cullant.
// SPDX-License-Identifier: GPL-3.0-or-later

package app.tauri.saf

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.os.storage.StorageManager
import android.provider.DocumentsContract
import android.provider.DocumentsContract.Document
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

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

    // Hand a document to an external app via an ACTION_VIEW chooser. The clip
    // stays where it is (SAF content URI); the launched app gets a temporary
    // read grant for the lifetime of that task. Used as the "open in an external
    // player" fallback when the in-app WebView can't decode a video.
    @Command
    fun openDocument(invoke: Invoke) {
        val args = invoke.parseArgs(OpenDocumentArgs::class.java)
        try {
            val uri = docUri(args.treeUri, args.documentId)
            val mime = args.mimeType ?: resolver.getType(uri) ?: "*/*"
            val view = Intent(Intent.ACTION_VIEW).apply {
                setDataAndType(uri, mime)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            val chooser = Intent.createChooser(view, null).apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            }
            activity.startActivity(chooser)
            val res = JSObject()
            res.put("ok", true)
            invoke.resolve(res)
        } catch (e: Exception) {
            invoke.reject(e.message ?: "failed to open document")
        }
    }

    private fun resolveDocId(invoke: Invoke, uri: Uri) {
        val res = JSObject()
        res.put("documentId", DocumentsContract.getDocumentId(uri))
        invoke.resolve(res)
    }
}
