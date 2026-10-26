package com.framecorder.app.sync

import android.app.Application
import android.content.Context
import android.os.Build
import android.os.PowerManager
import android.os.SystemClock
import android.provider.Settings
import com.framecorder.app.BuildConfig
import com.framecorder.core.Clip
import com.framecorder.core.Core
import com.framecorder.core.CoreException
import com.framecorder.core.Events
import com.framecorder.core.Forgot
import com.framecorder.core.FoundFrame
import com.framecorder.core.FrameInfo
import com.framecorder.core.FrameState
import com.framecorder.core.FrameStatus
import com.framecorder.core.RecordingPatch
import com.framecorder.core.RecordingSettings
import com.framecorder.core.RemoteState
import com.framecorder.core.SyncProgress
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update

/** The download running right now, with how fast it's going. */
data class Transfer(
    val fingerprint: String,
    val id: String,
    val name: String,
    val done: Long,
    val total: Long,
    /** Files waiting after this one. */
    val queued: Int,
    /** Files already done since this run of downloads started. */
    val finished: Int,
    val bytesPerSecond: Double,
) {
    val fraction: Float get() = if (total > 0) (done.toDouble() / total).toFloat().coerceIn(0f, 1f) else 1f
}

/** What a frame's tab is up to, and when we heard, so a recording's timer keeps counting. */
data class RemoteNow(val state: RemoteState, val at: Long = SystemClock.elapsedRealtime()) {
    fun elapsedMs(now: Long = SystemClock.elapsedRealtime()): Long = state.elapsedMs.toLong() + if (state.running) now - at else 0
}

/**
 * The app's side of the sync core: owns it, turns what it says into state
 * the UI collects, and holds a wake lock while a download runs. One per
 * process, made by [com.framecorder.app.FramecorderApp].
 */
class SyncHub(private val app: Application) : Events {
    private val _frames = MutableStateFlow<List<FrameStatus>>(emptyList())
    val frames: StateFlow<List<FrameStatus>> = _frames

    private val _clips = MutableStateFlow<List<Clip>>(emptyList())
    /** Everything synced, newest first. */
    val clips: StateFlow<List<Clip>> = _clips

    private val _transfer = MutableStateFlow<Transfer?>(null)
    val transfer: StateFlow<Transfer?> = _transfer

    private val _busy = MutableStateFlow(false)
    val busy: StateFlow<Boolean> = _busy

    private val _missing = MutableStateFlow<Set<String>>(emptySet())
    /** Keys of clips whose file was deleted on the phone since. */
    val missing: StateFlow<Set<String>> = _missing

    private val _remotes = MutableStateFlow<Map<String, RemoteNow>>(emptyMap())
    /** Per frame, what its tab is up to, while we're in touch with it. */
    val remotes: StateFlow<Map<String, RemoteNow>> = _remotes

    private val _about = MutableStateFlow<Map<String, FrameInfo>>(emptyMap())
    /** Per frame, its battery and storage, while we're in touch with it. */
    val about: StateFlow<Map<String, FrameInfo>> = _about

    private val _recordings = MutableStateFlow<Map<String, RecordingSettings>>(emptyMap())
    val recordings: StateFlow<Map<String, RecordingSettings>> = _recordings

    private val _updating = MutableStateFlow<Set<String>>(emptySet())
    /** Frames we asked to update, until they say they are. */
    val updating: StateFlow<Set<String>> = _updating

    private val _synced = MutableSharedFlow<Clip>(extraBufferCapacity = 16)
    /** Each clip as it lands, for notifications and the "new" badge. */
    val synced: SharedFlow<Clip> = _synced

    val gallery = MediaGallery(app)
    val finder = NsdFinder(app)

    private val wake = app.getSystemService(PowerManager::class.java)
        .newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "framecorder:download")
        .apply { setReferenceCounted(false) }

    // the download rate, smoothed
    private var lastSample: Pair<Long, Long>? = null
    private var lastId: String? = null
    private var smoothedRate = 0.0
    @Volatile private var finished = 0

    private val core: Core = Core(
        stateDir = stateDir(app).path,
        stagingDir = File(app.cacheDir, "downloads").path,
        deviceName = deviceName(app),
        app = "android/${BuildConfig.VERSION_NAME}",
        gallery = gallery,
        events = this,
        finder = finder,
    )

    init {
        _frames.value = core.statuses()
        _clips.value = core.clips()
    }

    /** Starts following every paired Frame. Safe to call again. */
    fun start() = core.start()

    /** Waits for transfers to stop and releases the download wake lock. */
    suspend fun stop() = core.stop()

    fun retryNow() = core.retryNow()

    /** [replaces] is a paired frame this one stands for; what came from it carries over. */
    suspend fun pair(addr: String, fingerprint: String?, code: String, replaces: String? = null): FrameStatus =
        core.pair(addr, fingerprint, code, replaces).also { took(replaces, it) }

    suspend fun pairLink(link: String, replaces: String? = null): FrameStatus =
        core.pairLink(link, replaces).also { took(replaces, it) }

    private suspend fun took(replaces: String?, paired: FrameStatus) {
        if (replaces == null || replaces == paired.fingerprint) return
        _frames.update { list -> list.filter { it.fingerprint != replaces } }
        _clips.value = core.clips()
        refreshMissing()
    }

    /** Who answers at a typed-in address, before pairing with it. */
    suspend fun identify(addr: String): FoundFrame = core.identify(addr)

    /** Asks every address on this Wi-Fi directly, for routers that block mDNS. */
    suspend fun sweep(seconds: Int): List<FoundFrame> = core.sweep(seconds.toUInt())

    /** Asks the frame to "record", "stop" or "clip"; fails with why not. */
    suspend fun command(fingerprint: String, what: String): RemoteState =
        core.command(fingerprint, what).also { remote(fingerprint, it) }

    /** The frame's recording settings, or null when its framecorder is older than them. */
    suspend fun recording(fingerprint: String): RecordingSettings? =
        core.recording(fingerprint).also { settings -> if (settings != null) recording(fingerprint, settings) }

    /** Saves them on the frame, for its next recording. */
    suspend fun setRecording(fingerprint: String, settings: RecordingPatch): RecordingSettings =
        core.setRecording(fingerprint, settings).also { recording(fingerprint, it) }

    /**
     * Has the frame install its framecorder update now. It counts as updating
     * from here until the frame says so itself, or says why it couldn't start.
     */
    suspend fun startUpdate(fingerprint: String) {
        _updating.update { it + fingerprint }
        try {
            core.startUpdate(fingerprint)
        } catch (e: CoreException) {
            _updating.update { it - fingerprint }
            throw e
        }
    }

    /** Null if the Frame answers at its last address, else why not. */
    suspend fun check(fingerprint: String): String? = try {
        core.check(fingerprint)
        null
    } catch (e: CoreException) {
        e.reason()
    }

    /** Looks which clips are gone from the gallery (deleted in another app, or here). */
    suspend fun refreshMissing() = withContext(Dispatchers.IO) {
        _missing.value = _clips.value.filterNot { gallery.exists(it.location) }.map { it.key }.toSet()
    }

    suspend fun unpair(fingerprint: String): Forgot {
        val result = core.unpair(fingerprint)
        _frames.update { list -> list.filter { it.fingerprint != fingerprint } }
        _recordings.update { it - fingerprint }
        _remotes.update { it - fingerprint }
        _about.update { it - fingerprint }
        _updating.update { it - fingerprint }
        return result
    }

    // the core calls these from its own threads

    override fun status(status: FrameStatus) {
        _frames.update { list ->
            val old = list.firstOrNull { it.fingerprint == status.fingerprint }
            // every retry starts with "connecting"; a frame that couldn't be reached
            // keeps showing that until a retry gets somewhere
            val shown = if (status.state == FrameState.CONNECTING && old != null && old.state != FrameState.CONNECTED) old else status
            if (old != null) list.map { if (it.fingerprint == status.fingerprint) shown else it } else list + shown
        }
        // once the frame says it's updating, or that there's nothing to update anymore, it speaks for itself
        if (status.update?.let { it.updating || !it.available } == true) _updating.update { it - status.fingerprint }
    }

    override fun progress(progress: SyncProgress) {
        val now = System.nanoTime()
        synchronized(this) {
            if (lastId != progress.id) {
                lastId = progress.id
                lastSample = now to progress.done
                smoothedRate = 0.0
            } else {
                val (t0, d0) = lastSample ?: (now to progress.done)
                val seconds = (now - t0) / 1e9
                if (seconds >= 0.4) {
                    val rate = (progress.done - d0).coerceAtLeast(0) / seconds
                    smoothedRate = if (smoothedRate == 0.0) rate else smoothedRate * 0.6 + rate * 0.4
                    lastSample = now to progress.done
                }
            }
        }
        _transfer.value = Transfer(
            fingerprint = progress.fingerprint,
            id = progress.id,
            name = progress.name,
            done = progress.done,
            total = progress.total,
            queued = progress.queued,
            finished = finished,
            bytesPerSecond = smoothedRate,
        )
    }

    override fun synced(clip: Clip) {
        _clips.update { list -> listOf(clip) + list.filter { it.key != clip.key } }
        if (_busy.value) finished++
        _synced.tryEmit(clip)
    }

    override fun busy(busy: Boolean) {
        if (busy && !_busy.value) finished = 0
        _busy.value = busy
        if (busy) {
            // bounded, in case "done" never comes because something died
            wake.acquire(30 * 60 * 1000L)
        } else {
            _transfer.value = null
            if (wake.isHeld) wake.release()
        }
    }

    override fun remote(host: String, state: RemoteState?) {
        _remotes.update { if (state == null) it - host else it + (host to RemoteNow(state)) }
    }

    override fun recording(host: String, settings: RecordingSettings) {
        _recordings.update { it + (host to settings) }
    }

    override fun about(host: String, info: FrameInfo?) {
        _about.update { if (info == null) it - host else it + (host to info) }
    }

    override fun removed(host: String, id: String) {
        // gone from the frame; the copy here stays
    }
}

/** What the error says, without the generated "reason=" wrapping. */
fun CoreException.reason(): String = when (this) {
    is CoreException.Failed -> reason
}

/**
 * Where the core keeps its pairings and what it has synced. The old app (0.1.x,
 * same package, Tauri) kept them straight in the app's data folder, so the
 * first start after updating from it moves them here. Without that, the update
 * unpairs the frame, and pairing again downloads every clip a second time.
 */
private fun stateDir(context: Context): File {
    val dir = File(context.filesDir, "sync")
    val old = context.dataDir
    if (!File(dir, "hosts.json").exists() && File(old, "hosts.json").exists()) {
        dir.mkdirs()
        for (name in listOf("hosts.json", "index.json")) {
            File(old, name).takeIf { it.exists() }?.renameTo(File(dir, name))
        }
    }
    return dir
}

/** What this phone calls itself when pairing: its name in settings, else the model. */
private fun deviceName(context: Context): String {
    val named = runCatching { Settings.Global.getString(context.contentResolver, Settings.Global.DEVICE_NAME) }.getOrNull()
    if (!named.isNullOrBlank()) return named
    val maker = Build.MANUFACTURER.replaceFirstChar { it.uppercase() }
    return if (Build.MODEL.startsWith(Build.MANUFACTURER, ignoreCase = true)) Build.MODEL else "$maker ${Build.MODEL}"
}
