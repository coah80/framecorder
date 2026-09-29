package com.framecorder.framesync

import android.app.Activity
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Environment
import android.os.PowerManager
import android.provider.MediaStore
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.io.FileInputStream

@InvokeArg
class BusyArgs {
    var busy: Boolean = false
}

@InvokeArg
class SaveArgs {
    lateinit var path: String
    lateinit var name: String
    var subdir: String = ""
}

@InvokeArg
class UriArgs {
    lateinit var uri: String
}

@TauriPlugin
class FrameSyncPlugin(private val activity: Activity) : Plugin(activity) {
    private val context: Context = activity.applicationContext
    private var multicast: WifiManager.MulticastLock? = null
    private var wake: PowerManager.WakeLock? = null

    @Command
    fun startService(invoke: Invoke) {
        try {
            if (multicast == null) {
                val wifi = context.getSystemService(Context.WIFI_SERVICE) as WifiManager
                multicast = wifi.createMulticastLock("framecorder-mdns").apply {
                    setReferenceCounted(false)
                    acquire()
                }
            }
            ContextCompat.startForegroundService(context, Intent(context, SyncService::class.java))
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("couldn't start syncing in the background: ${e.message}")
        }
    }

    @Command
    fun stopService(invoke: Invoke) {
        context.stopService(Intent(context, SyncService::class.java))
        multicast?.release()
        multicast = null
        invoke.resolve()
    }

    @Command
    fun setBusy(invoke: Invoke) {
        val args = invoke.parseArgs(BusyArgs::class.java)
        if (args.busy) {
            if (wake == null) {
                val power = context.getSystemService(Context.POWER_SERVICE) as PowerManager
                wake = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "framecorder:download").apply {
                    setReferenceCounted(false)
                }
            }
            // bounded, in case we never hear "done" because something died
            wake?.acquire(30 * 60 * 1000L)
        } else {
            wake?.let { if (it.isHeld) it.release() }
        }
        invoke.resolve()
    }

    @Command
    fun saveToGallery(invoke: Invoke) {
        val args = invoke.parseArgs(SaveArgs::class.java)
        Thread {
            try {
                val ret = JSObject()
                ret.put("uri", save(args).toString())
                invoke.resolve(ret)
            } catch (e: Exception) {
                invoke.reject("couldn't save to the gallery: ${e.message}")
            }
        }.start()
    }

    private fun save(args: SaveArgs): Uri {
        val source = File(args.path)
        val folder = listOf(Environment.DIRECTORY_MOVIES, "framecorder", args.subdir)
            .filter { it.isNotEmpty() }
            .joinToString("/")
        val resolver = context.contentResolver
        val values = ContentValues().apply {
            put(MediaStore.Video.Media.DISPLAY_NAME, args.name)
            put(MediaStore.Video.Media.MIME_TYPE, "video/mp4")
            put(MediaStore.Video.Media.RELATIVE_PATH, folder)
            put(MediaStore.Video.Media.IS_PENDING, 1)
        }
        val collection = MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val uri = resolver.insert(collection, values) ?: throw IllegalStateException("MediaStore said no")
        try {
            resolver.openOutputStream(uri)?.use { out ->
                FileInputStream(source).use { it.copyTo(out, 1 shl 20) }
            } ?: throw IllegalStateException("couldn't open the new file")
            values.clear()
            values.put(MediaStore.Video.Media.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
        } catch (e: Exception) {
            resolver.delete(uri, null, null)
            throw e
        }
        source.delete()
        return uri
    }

    @Command
    fun open(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(Uri.parse(args.uri), "video/mp4")
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        }
        try {
            activity.startActivity(intent)
            invoke.resolve()
        } catch (e: Exception) {
            invoke.reject("nothing on this phone can play it")
        }
    }

    @Command
    fun share(invoke: Invoke) {
        val args = invoke.parseArgs(UriArgs::class.java)
        val send = Intent(Intent.ACTION_SEND).apply {
            type = "video/mp4"
            putExtra(Intent.EXTRA_STREAM, Uri.parse(args.uri))
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
        activity.startActivity(Intent.createChooser(send, "Share clip"))
        invoke.resolve()
    }

    @Command
    fun deviceName(invoke: Invoke) {
        val maker = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }
        val name = if (Build.MODEL.startsWith(Build.MANUFACTURER, ignoreCase = true)) Build.MODEL else "$maker ${Build.MODEL}"
        val ret = JSObject()
        ret.put("name", name)
        invoke.resolve(ret)
    }
}
