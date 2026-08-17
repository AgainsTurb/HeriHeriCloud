package com.heriheri.gstreamerplayer

import android.app.Activity
import android.app.Dialog
import android.graphics.Color
import android.graphics.drawable.ColorDrawable
import android.graphics.drawable.GradientDrawable
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.ViewGroup
import android.view.View
import android.view.Window
import android.widget.Button
import android.widget.CheckBox
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.SeekBar
import android.widget.Spinner
import android.widget.ArrayAdapter
import android.widget.TextView
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.freedesktop.gstreamer.GStreamer
import org.json.JSONArray

@InvokeArg class ProcessorRequest { var kind: String = "passthrough" }
@InvokeArg class OpenRequest {
    var uri: String = ""
    var title: String = "Media Player"
    var isAudio: Boolean = false
    var startPositionMs: Long? = null
    var expectedGeneration: Long? = null
    var processor: ProcessorRequest = ProcessorRequest()
}
@InvokeArg class OpenInvokeArgs { var request: OpenRequest = OpenRequest() }
@InvokeArg class SeekRequest { var positionMs: Long = 0 }
@InvokeArg class VolumeRequest { var volume: Double = 1.0 }
@InvokeArg class MutedRequest { var muted: Boolean = false }
@InvokeArg class RateRequest { var rate: Double = 1.0 }
@InvokeArg class LoopingRequest { var looping: Boolean = false }
@InvokeArg class StopRequest { var expectedGeneration: Long? = null }
@InvokeArg class TrackRequest { var index: Int = -1 }
@InvokeArg class SubtitleUriRequest { var uri: String? = null }

@android.annotation.TargetApi(Build.VERSION_CODES.TIRAMISU)
private object Api33BackHandler {
    fun register(dialog: Dialog, action: () -> Unit): Any {
        val callback = android.window.OnBackInvokedCallback { action() }
        dialog.onBackInvokedDispatcher.registerOnBackInvokedCallback(
            android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,
            callback,
        )
        return callback
    }

    fun unregister(dialog: Dialog, callback: Any) {
        dialog.onBackInvokedDispatcher.unregisterOnBackInvokedCallback(
            callback as android.window.OnBackInvokedCallback,
        )
    }
}

@TauriPlugin
class GStreamerPlayerPlugin(private val activity: Activity) : Plugin(activity), SurfaceHolder.Callback {
    private var dialog: Dialog? = null
    private var surfaceView: SurfaceView? = null
    private var pendingRequest: OpenRequest? = null
    private var seekBar: SeekBar? = null
    private var loadingIndicator: ProgressBar? = null
    private var controlsView: View? = null
    private var backInvokedCallback: Any? = null
    private val handler = Handler(Looper.getMainLooper())
    private var generation = 0L
    private var volume = 1.0
    private var muted = false
    private var rate = 1.0
    private var looping = false
    private var status = "stopped"
    private var externalSubtitleUri: String? = null
    private var chromeViews: List<View> = emptyList()
    private var nativeRuntimeReady = false
    private val hideChrome = Runnable {
        if (status == "playing") chromeViews.forEach { it.animate().alpha(0f).setDuration(220).start() }
    }

    private external fun nativeOpen(uri: String, surface: Surface): Boolean
    private external fun nativePlay()
    private external fun nativePause()
    private external fun nativeStop()
    private external fun nativeSeek(positionMs: Long): Boolean
    private external fun nativeSetVolume(volume: Double)
    private external fun nativeSetMuted(muted: Boolean)
    private external fun nativeSetRate(rate: Double): Boolean
    private external fun nativeSetLooping(looping: Boolean)
    private external fun nativeAudioTrackCount(): Int
    private external fun nativeSubtitleTrackCount(): Int
    private external fun nativeCurrentAudioTrack(): Int
    private external fun nativeCurrentSubtitleTrack(): Int
    private external fun nativeSelectAudioTrack(index: Int): Boolean
    private external fun nativeSelectSubtitleTrack(index: Int): Boolean
    private external fun nativeSetSubtitleUri(uri: String?): Boolean
    private external fun nativePosition(): Long
    private external fun nativeDuration(): Long
    private external fun nativeIsPlaying(): Boolean

    override fun load(webView: WebView) {
        super.load(webView)
    }

    // Loading the monolithic GStreamer runtime during WebView creation blocks application startup.
    // Initialize it only when the user actually opens media.
    private fun ensureNativeRuntime(): String? {
        if (nativeRuntimeReady) return null
        return try {
            System.loadLibrary("gstreamer_android")
            System.loadLibrary("heri_gstreamer_player")
            GStreamer.init(activity)
            nativeRuntimeReady = true
            null
        } catch (error: Throwable) {
            error.message ?: error.javaClass.simpleName
        }
    }

    @Command fun open(invoke: Invoke) {
        val request = invoke.parseArgs(OpenInvokeArgs::class.java).request
        if (request.processor.kind != "passthrough") return invoke.reject("No ONNX frame processor is installed")
        activity.runOnUiThread {
            if (request.uri.isBlank()) {
                closePlayer()
                pendingRequest = request
                generation += 1
                status = "preparing"
                showPlayer(request)
                invoke.resolve(openResponse(opened = false))
                return@runOnUiThread
            }

            request.expectedGeneration?.let { expected ->
                if (dialog == null || generation != expected || pendingRequest?.uri?.isNotBlank() == true) {
                    invoke.resolve(openResponse(opened = false))
                    return@runOnUiThread
                }
            }
            ensureNativeRuntime()?.let { message ->
                invoke.reject("Unable to initialize native GStreamer: $message")
                return@runOnUiThread
            }

            if (request.expectedGeneration != null) {
                pendingRequest = request
                val surface = surfaceView?.holder?.surface
                if (surface != null && surface.isValid && !startPlayback(request, surface)) {
                    invoke.reject("Unable to start native GStreamer playback")
                    return@runOnUiThread
                }
                invoke.resolve(openResponse(opened = true))
                return@runOnUiThread
            }

            closePlayer()
            pendingRequest = request
            generation += 1
            showPlayer(request)
            invoke.resolve(openResponse(opened = true))
        }
    }

    private fun openResponse(opened: Boolean) = JSObject().apply {
        put("generation", generation)
        put("rendererMode", "native-surface")
        put("opened", opened)
    }

    private fun startPlayback(request: OpenRequest, surface: Surface): Boolean {
        if (!nativeRuntimeReady || request.uri.isBlank()) return false
        if (!nativeOpen(request.uri, surface)) {
            status = "stopped"
            return false
        }
        status = "playing"
        loadingIndicator?.visibility = View.GONE
        controlsView?.visibility = View.VISIBLE
        nativeSetVolume(volume)
        nativeSetMuted(muted)
        nativeSetLooping(looping)
        request.startPositionMs?.let { nativeSeek(it) }
        revealChrome()
        return true
    }

    private fun showPlayer(request: OpenRequest) {
        val playerDialog = object : Dialog(activity, android.R.style.Theme_Black_NoTitleBar_Fullscreen) {
            @Deprecated("Android routes legacy back events here below API 33")
            override fun onBackPressed() {
                closePlayer()
            }
        }
        playerDialog.requestWindowFeature(Window.FEATURE_NO_TITLE)
        playerDialog.setCancelable(true)
        playerDialog.setCanceledOnTouchOutside(false)
        playerDialog.window?.setBackgroundDrawable(ColorDrawable(Color.BLACK))
        val root = FrameLayout(activity)
        val video = SurfaceView(activity)
        video.holder.addCallback(this)
        root.addView(video, FrameLayout.LayoutParams(-1, -1))

        val loading = ProgressBar(activity).apply {
            isIndeterminate = true
            visibility = if (request.uri.isBlank()) View.VISIBLE else View.GONE
        }
        root.addView(loading, FrameLayout.LayoutParams(-2, -2, Gravity.CENTER))

        fun glassBackground(radius: Float = 28f) = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = radius
            setColor(0x6825364F)
            setStroke(1, 0x66FFFFFF)
        }
        fun styledButton(label: String, action: () -> Unit) = Button(activity).apply {
            text = label
            setTextColor(Color.WHITE)
            background = glassBackground(22f)
            minWidth = 0
            setPadding(18, 4, 18, 4)
            setOnClickListener { action(); revealChrome() }
        }

        val titleBar = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
            background = glassBackground(34f)
            elevation = 18f
            setPadding(22, 7, 7, 7)
        }
        titleBar.addView(TextView(activity).apply {
            text = request.title
            setTextColor(Color.WHITE)
            maxLines = 1
            ellipsize = android.text.TextUtils.TruncateAt.END
        }, LinearLayout.LayoutParams(0, -2, 1f))
        titleBar.addView(styledButton("×") { closePlayer() })
        root.addView(titleBar, FrameLayout.LayoutParams(-1, -2, Gravity.TOP).apply {
            setMargins(18, 18, 18, 0)
        })

        val bottom = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            background = glassBackground(38f)
            elevation = 20f
            setPadding(18, 8, 18, 12)
            visibility = if (request.uri.isBlank()) View.INVISIBLE else View.VISIBLE
        }
        val timeline = SeekBar(activity).apply {
            max = 1000
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(bar: SeekBar, progress: Int, fromUser: Boolean) = Unit
                override fun onStartTrackingTouch(bar: SeekBar) = Unit
                override fun onStopTrackingTouch(bar: SeekBar) {
                    val duration = nativeDuration()
                    if (duration > 0) nativeSeek(duration * bar.progress / 1000L)
                }
            })
        }
        seekBar = timeline
        bottom.addView(timeline, LinearLayout.LayoutParams(-1, -2))

        val primaryControls = LinearLayout(activity).apply { gravity = Gravity.CENTER }
        lateinit var playPause: Button
        playPause = styledButton("Ⅱ") {
            if (status == "playing") {
                nativePause(); status = "paused"; playPause.text = "▶"
            } else {
                nativePlay(); status = "playing"; playPause.text = "Ⅱ"
            }
        }
        primaryControls.addView(styledButton("↶ 10") { nativeSeek((nativePosition() - 10_000).coerceAtLeast(0)) })
        primaryControls.addView(playPause)
        primaryControls.addView(styledButton("10 ↷") {
            val duration = nativeDuration()
            val target = nativePosition() + 10_000
            nativeSeek(if (duration > 0) target.coerceAtMost(duration) else target)
        })
        primaryControls.addView(styledButton("Audio") {
            val count = nativeAudioTrackCount()
            if (count > 0) nativeSelectAudioTrack((nativeCurrentAudioTrack() + 1).mod(count))
        })
        primaryControls.addView(styledButton("CC") {
            val count = nativeSubtitleTrackCount()
            val next = if (nativeCurrentSubtitleTrack() + 1 >= count) -1 else nativeCurrentSubtitleTrack() + 1
            nativeSelectSubtitleTrack(next)
        })

        val speeds = arrayOf("0.25x", "0.5x", "0.75x", "1x", "1.25x", "1.5x", "2x", "3x", "4x")
        primaryControls.addView(Spinner(activity).apply {
            adapter = ArrayAdapter(activity, android.R.layout.simple_spinner_dropdown_item, speeds)
            setSelection(3)
            background = glassBackground(22f)
            onItemSelectedListener = object : android.widget.AdapterView.OnItemSelectedListener {
                override fun onNothingSelected(parent: android.widget.AdapterView<*>?) = Unit
                override fun onItemSelected(parent: android.widget.AdapterView<*>?, view: android.view.View?, position: Int, id: Long) {
                    val selected = speeds[position].removeSuffix("x").toDouble()
                    if (selected != rate && nativeSetRate(selected)) rate = selected
                }
            }
        })
        primaryControls.addView(CheckBox(activity).apply {
            text = "Mute"
            setTextColor(Color.WHITE)
            setOnCheckedChangeListener { _, checked -> muted = checked; nativeSetMuted(checked) }
        })
        primaryControls.addView(CheckBox(activity).apply {
            text = "Repeat"
            setTextColor(Color.WHITE)
            setOnCheckedChangeListener { _, checked -> looping = checked; nativeSetLooping(checked) }
        })
        bottom.addView(primaryControls, LinearLayout.LayoutParams(-1, -2))
        root.addView(bottom, FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM).apply {
            setMargins(18, 0, 18, 22)
        })

        chromeViews = listOf(titleBar, bottom)
        root.setOnTouchListener { _, _ -> revealChrome(); false }
        video.setOnTouchListener { _, _ -> revealChrome(); false }

        playerDialog.setContentView(root)
        playerDialog.setOnDismissListener { finishPlayer() }
        surfaceView = video
        loadingIndicator = loading
        controlsView = bottom
        dialog = playerDialog
        playerDialog.show()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            backInvokedCallback = Api33BackHandler.register(playerDialog) { closePlayer() }
        }
        handler.post(progressUpdater)
        revealChrome()
    }

    private fun revealChrome() {
        handler.removeCallbacks(hideChrome)
        chromeViews.forEach { it.animate().alpha(1f).setDuration(120).start() }
        if (status == "playing") handler.postDelayed(hideChrome, 2800)
    }

    private val progressUpdater = object : Runnable {
        override fun run() {
            if (nativeRuntimeReady && pendingRequest?.uri?.isNotBlank() == true) {
                val duration = nativeDuration()
                val position = nativePosition()
                if (duration > 0 && position >= 0) seekBar?.progress = (position * 1000L / duration).toInt()
            }
            if (dialog != null) handler.postDelayed(this, 500)
        }
    }

    private fun finishPlayer() {
        handler.removeCallbacks(progressUpdater)
        handler.removeCallbacks(hideChrome)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            backInvokedCallback?.let { callback ->
                dialog?.let { current -> Api33BackHandler.unregister(current, callback) }
            }
        }
        backInvokedCallback = null
        if (nativeRuntimeReady) nativeStop()
        surfaceView = null
        dialog = null
        pendingRequest = null
        seekBar = null
        loadingIndicator = null
        controlsView = null
        status = "stopped"
        rate = 1.0
        externalSubtitleUri = null
        chromeViews = emptyList()
    }

    private fun closePlayer() {
        val current = dialog
        current?.setOnDismissListener(null)
        finishPlayer()
        current?.dismiss()
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        if (surfaceView?.holder !== holder) return
        val request = pendingRequest ?: return
        if (request.uri.isNotBlank() && nativeRuntimeReady) startPlayback(request, holder.surface)
    }
    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) = Unit
    override fun surfaceDestroyed(holder: SurfaceHolder) {
        if (surfaceView?.holder === holder && nativeRuntimeReady) nativeStop()
    }

    @Command fun play(invoke: Invoke) { nativePlay(); status = "playing"; invoke.resolve() }
    @Command fun pause(invoke: Invoke) { nativePause(); status = "paused"; invoke.resolve() }
    @Command fun stop(invoke: Invoke) {
        val expectedGeneration = invoke.parseArgs(StopRequest::class.java).expectedGeneration
        activity.runOnUiThread {
            val closed = dialog != null && (expectedGeneration == null || expectedGeneration == generation)
            if (closed) closePlayer()
            invoke.resolve(JSObject().apply {
                put("generation", generation)
                put("closed", closed)
            })
        }
    }
    @Command fun seek(invoke: Invoke) {
        val request = invoke.parseArgs(SeekRequest::class.java)
        if (nativeSeek(request.positionMs)) invoke.resolve() else invoke.reject("Seek failed")
    }
    @Command fun setVolume(invoke: Invoke) {
        val request = invoke.parseArgs(VolumeRequest::class.java)
        if (request.volume !in 0.0..1.0) return invoke.reject("Volume must be between 0 and 1")
        volume = request.volume; nativeSetVolume(volume); invoke.resolve()
    }
    @Command fun setMuted(invoke: Invoke) {
        muted = invoke.parseArgs(MutedRequest::class.java).muted; nativeSetMuted(muted); invoke.resolve()
    }
    @Command fun setRate(invoke: Invoke) {
        val requested = invoke.parseArgs(RateRequest::class.java).rate
        if (requested !in 0.25..4.0) return invoke.reject("Playback rate must be between 0.25 and 4")
        if (nativeSetRate(requested)) { rate = requested; invoke.resolve() } else invoke.reject("Rate change failed")
    }
    @Command fun setLooping(invoke: Invoke) {
        looping = invoke.parseArgs(LoopingRequest::class.java).looping; nativeSetLooping(looping); invoke.resolve()
    }
    @Command fun selectAudioTrack(invoke: Invoke) {
        val index = invoke.parseArgs(TrackRequest::class.java).index
        if (index in 0 until nativeAudioTrackCount() && nativeSelectAudioTrack(index)) invoke.resolve()
        else invoke.reject("Audio track index is out of range")
    }
    @Command fun selectSubtitleTrack(invoke: Invoke) {
        val index = invoke.parseArgs(TrackRequest::class.java).index
        if (index in -1 until nativeSubtitleTrackCount() && nativeSelectSubtitleTrack(index)) invoke.resolve()
        else invoke.reject("Subtitle track index is out of range")
    }
    @Command fun setSubtitleUri(invoke: Invoke) {
        val uri = invoke.parseArgs(SubtitleUriRequest::class.java).uri
        if (nativeSetSubtitleUri(uri)) { externalSubtitleUri = uri; invoke.resolve() }
        else invoke.reject("No media is open")
    }

    private fun tracks(count: Int, selected: Int, label: String): JSONArray = JSONArray().apply {
        repeat(count.coerceAtLeast(0)) { index ->
            put(JSObject().apply {
                put("index", index); put("label", "$label ${index + 1}")
                put("language", null); put("codec", null); put("selected", index == selected)
            })
        }
    }

    @Command fun getState(invoke: Invoke) {
        invoke.resolve(JSObject().apply {
            val hasNativePlayer = nativeRuntimeReady && pendingRequest?.uri?.isNotBlank() == true
            val currentStatus = when {
                dialog == null -> "stopped"
                !hasNativePlayer -> "preparing"
                nativeIsPlaying() -> "playing"
                else -> "paused"
            }
            put("generation", generation); put("status", currentStatus)
            put("positionMs", if (hasNativePlayer) nativePosition().takeIf { it >= 0 } else null)
            put("durationMs", if (hasNativePlayer) nativeDuration().takeIf { it >= 0 } else null)
            put("volume", volume); put("muted", muted); put("rate", rate); put("looping", looping)
            put("bufferingPercent", null); put("title", pendingRequest?.title)
            put("audioTracks", if (hasNativePlayer) tracks(nativeAudioTrackCount(), nativeCurrentAudioTrack(), "Audio") else JSONArray())
            put("subtitleTracks", if (hasNativePlayer) tracks(nativeSubtitleTrackCount(), nativeCurrentSubtitleTrack(), "Subtitle") else JSONArray())
            put("externalSubtitleUri", externalSubtitleUri)
        })
    }
    @Command fun capabilities(invoke: Invoke) {
        invoke.resolve(JSObject().apply {
            put("protocolVersion", 2); put("frameProcessorApiVersion", 1)
            put("engine", "GStreamer"); put("nativeVideo", true)
            put("playbackRates", JSONArray(listOf(0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4)))
            put("embeddedSubtitles", true); put("externalSubtitles", true); put("multipleAudioTracks", true)
            put("aiProcessors", JSONArray(listOf("passthrough")))
        })
    }
}
