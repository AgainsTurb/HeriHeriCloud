#include <android/native_window.h>
#include <android/native_window_jni.h>
#include <android/log.h>
#include <gst/gst.h>
#include <gst/video/video-frame.h>
#include <jni.h>
#include <pthread.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

typedef struct {
  pthread_mutex_t mutex;
  pthread_mutex_t window_mutex;
  GstElement *playbin;
  GstBus *bus;
  GThread *bus_thread;
  ANativeWindow *window;
  gint stop_requested;
  gint desired_playing;
  gint buffering_percent;
  gint looping;
  gint rate_milli;
} HeriPlayer;

static HeriPlayer player = {
    .mutex = PTHREAD_MUTEX_INITIALIZER,
    .window_mutex = PTHREAD_MUTEX_INITIALIZER,
    .buffering_percent = 100,
    .rate_milli = 1000,
};

#define HERI_GST_LOG_TAG "HeriGStreamerPlayer"
#define HERI_BUFFER_DURATION (8 * GST_SECOND)
#define HERI_BUFFER_SIZE (8 * 1024 * 1024)
#define HERI_RING_BUFFER_SIZE (32 * 1024 * 1024)
#define HERI_PLAY_FLAG_DOWNLOAD (1u << 7)
#define HERI_PLAY_FLAG_BUFFERING (1u << 8)

static gboolean seek_at_rate(GstElement *playbin, gint64 position,
                             gdouble rate) {
  GstSeekFlags fast_flags = GST_SEEK_FLAG_FLUSH | GST_SEEK_FLAG_KEY_UNIT |
                            GST_SEEK_FLAG_SNAP_NEAREST;
  if (gst_element_seek(playbin, rate, GST_FORMAT_TIME, fast_flags,
                       GST_SEEK_TYPE_SET, position, GST_SEEK_TYPE_NONE,
                       GST_CLOCK_TIME_NONE)) {
    return TRUE;
  }

  __android_log_print(ANDROID_LOG_WARN, HERI_GST_LOG_TAG,
                      "Keyframe seek was rejected; retrying accurately");
  return gst_element_seek(playbin, rate, GST_FORMAT_TIME,
                          GST_SEEK_FLAG_FLUSH | GST_SEEK_FLAG_ACCURATE,
                          GST_SEEK_TYPE_SET, position, GST_SEEK_TYPE_NONE,
                          GST_CLOCK_TIME_NONE);
}

static gpointer run_bus(gpointer data) {
  HeriPlayer *state = (HeriPlayer *)data;
  while (!g_atomic_int_get(&state->stop_requested)) {
    GstMessage *message = gst_bus_timed_pop_filtered(
        state->bus, 250 * GST_MSECOND,
        GST_MESSAGE_EOS | GST_MESSAGE_ERROR | GST_MESSAGE_BUFFERING);
    if (message == NULL) continue;
    if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_BUFFERING) {
      gint percent = 100;
      gst_message_parse_buffering(message, &percent);
      percent = CLAMP(percent, 0, 100);
      g_atomic_int_set(&state->buffering_percent, percent);
      if (percent < 100) {
        if (g_atomic_int_get(&state->desired_playing))
          gst_element_set_state(state->playbin, GST_STATE_PAUSED);
      } else if (g_atomic_int_get(&state->desired_playing)) {
        gst_element_set_state(state->playbin, GST_STATE_PLAYING);
      }
    } else if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_EOS &&
               g_atomic_int_get(&state->looping)) {
      gdouble rate = g_atomic_int_get(&state->rate_milli) / 1000.0;
      seek_at_rate(state->playbin, 0, rate);
      if (g_atomic_int_get(&state->desired_playing))
        gst_element_set_state(state->playbin, GST_STATE_PLAYING);
    } else {
      if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_ERROR) {
        GError *error = NULL;
        gchar *debug = NULL;
        gst_message_parse_error(message, &error, &debug);
        __android_log_print(ANDROID_LOG_ERROR, HERI_GST_LOG_TAG,
                            "Playback error: %s%s%s%s",
                            error != NULL ? error->message : "Unknown error",
                            debug != NULL ? " (" : "",
                            debug != NULL ? debug : "",
                            debug != NULL ? ")" : "");
        g_clear_error(&error);
        g_free(debug);
      }
      g_atomic_int_set(&state->desired_playing, FALSE);
    }
    gst_message_unref(message);
  }
  return NULL;
}

static GstFlowReturn render_sample(GstElement *sink, gpointer data) {
  HeriPlayer *state = (HeriPlayer *)data;
  GstSample *sample = NULL;
  g_signal_emit_by_name(sink, "pull-sample", &sample);
  if (sample == NULL) return GST_FLOW_EOS;

  GstCaps *caps = gst_sample_get_caps(sample);
  GstBuffer *buffer = gst_sample_get_buffer(sample);
  GstVideoInfo info;
  GstVideoFrame frame;
  memset(&info, 0, sizeof(info));
  memset(&frame, 0, sizeof(frame));

  if (caps == NULL || buffer == NULL ||
      !gst_video_info_from_caps(&info, caps) ||
      GST_VIDEO_INFO_FORMAT(&info) != GST_VIDEO_FORMAT_RGBA ||
      !gst_video_frame_map(&frame, &info, buffer, GST_MAP_READ)) {
    __android_log_print(ANDROID_LOG_ERROR, HERI_GST_LOG_TAG,
                        "Unable to map a decoded RGBA video frame");
    gst_sample_unref(sample);
    return GST_FLOW_ERROR;
  }

  ANativeWindow *window = NULL;
  pthread_mutex_lock(&state->window_mutex);
  if (!g_atomic_int_get(&state->stop_requested) && state->window != NULL) {
    window = state->window;
    ANativeWindow_acquire(window);
  }
  pthread_mutex_unlock(&state->window_mutex);
  ANativeWindow_Buffer output;
  memset(&output, 0, sizeof(output));
  if (window == NULL || ANativeWindow_lock(window, &output, NULL) != 0) {
    __android_log_print(ANDROID_LOG_ERROR, HERI_GST_LOG_TAG,
                        "Unable to lock the Android RGBA video surface");
    gst_video_frame_unmap(&frame);
    gst_sample_unref(sample);
    if (window != NULL) ANativeWindow_release(window);
    return g_atomic_int_get(&state->stop_requested) ? GST_FLOW_FLUSHING
                                                    : GST_FLOW_ERROR;
  }
  if (output.bits == NULL || output.width <= 0 || output.height <= 0 ||
      output.format != WINDOW_FORMAT_RGBA_8888) {
    __android_log_print(ANDROID_LOG_ERROR, HERI_GST_LOG_TAG,
                        "Android returned an invalid RGBA video buffer");
    ANativeWindow_unlockAndPost(window);
    gst_video_frame_unmap(&frame);
    gst_sample_unref(sample);
    ANativeWindow_release(window);
    return GST_FLOW_ERROR;
  }

  uint32_t *destination = (uint32_t *)output.bits;
  for (int y = 0; y < output.height; y++) {
    uint32_t *row = destination + (size_t)y * output.stride;
    for (int x = 0; x < output.width; x++) row[x] = 0xff000000u;
  }

  const int source_width = GST_VIDEO_INFO_WIDTH(&info);
  const int source_height = GST_VIDEO_INFO_HEIGHT(&info);
  if (source_width <= 0 || source_height <= 0) {
    ANativeWindow_unlockAndPost(window);
    gst_video_frame_unmap(&frame);
    gst_sample_unref(sample);
    ANativeWindow_release(window);
    return GST_FLOW_ERROR;
  }
  const int pixel_aspect_n = MAX(GST_VIDEO_INFO_PAR_N(&info), 1);
  const int pixel_aspect_d = MAX(GST_VIDEO_INFO_PAR_D(&info), 1);
  int target_width = output.width;
  int target_height = (int)((int64_t)source_height * target_width *
                            pixel_aspect_d /
                            ((int64_t)source_width * pixel_aspect_n));
  if (target_height > output.height) {
    target_height = output.height;
    target_width = (int)((int64_t)source_width * target_height *
                         pixel_aspect_n /
                         ((int64_t)source_height * pixel_aspect_d));
  }
  target_width = MAX(target_width, 1);
  target_height = MAX(target_height, 1);
  const int target_x = (output.width - target_width) / 2;
  const int target_y = (output.height - target_height) / 2;
  const uint8_t *source = GST_VIDEO_FRAME_PLANE_DATA(&frame, 0);
  const int source_stride = GST_VIDEO_FRAME_PLANE_STRIDE(&frame, 0);

  if (source_width == output.width && source_height == output.height &&
      pixel_aspect_n == pixel_aspect_d) {
    for (int y = 0; y < source_height; y++) {
      const uint8_t *source_row =
          source + (ptrdiff_t)y * source_stride;
      uint8_t *target_row =
          (uint8_t *)(destination + (size_t)y * output.stride);
      memcpy(target_row, source_row, (size_t)source_width * 4);
    }
  } else {
    for (int y = 0; y < target_height; y++) {
      const int source_y = (int)((int64_t)y * source_height / target_height);
      const uint32_t *source_row = (const uint32_t *)(
          source + (ptrdiff_t)source_y * source_stride);
      uint32_t *target_row = destination +
          (size_t)(target_y + y) * output.stride + target_x;
      for (int x = 0; x < target_width; x++) {
        const int source_x = (int)((int64_t)x * source_width / target_width);
        target_row[x] = source_row[source_x];
      }
    }
  }

  const int post_result = ANativeWindow_unlockAndPost(window);
  gst_video_frame_unmap(&frame);
  gst_sample_unref(sample);
  ANativeWindow_release(window);
  if (post_result != 0) {
    __android_log_print(ANDROID_LOG_ERROR, HERI_GST_LOG_TAG,
                        "Unable to post a decoded frame to the Android surface");
    return g_atomic_int_get(&state->stop_requested) ? GST_FLOW_FLUSHING
                                                    : GST_FLOW_ERROR;
  }
  return GST_FLOW_OK;
}

static GstElement *create_android_video_sink(HeriPlayer *state) {
  GError *error = NULL;
  GstElement *sink_bin = gst_parse_bin_from_description(
      "videoconvert ! appsink name=heriheri-android-video-sink", TRUE,
      &error);
  if (sink_bin == NULL) {
    __android_log_print(ANDROID_LOG_ERROR, HERI_GST_LOG_TAG,
                        "Unable to create the Android video sink: %s",
                        error != NULL ? error->message : "Unknown error");
    g_clear_error(&error);
    return NULL;
  }
  g_clear_error(&error);

  GstElement *element = gst_bin_get_by_name(
      GST_BIN(sink_bin), "heriheri-android-video-sink");
  if (element == NULL) {
    gst_object_unref(sink_bin);
    return NULL;
  }

  GstCaps *caps = gst_caps_new_simple("video/x-raw", "format",
                                     G_TYPE_STRING, "RGBA", NULL);
  g_object_set(element, "caps", caps, "emit-signals", TRUE,
               "max-buffers", (guint)2, "drop", TRUE,
               "wait-on-eos", FALSE, NULL);
  gst_caps_unref(caps);
  g_signal_connect(element, "new-sample", G_CALLBACK(render_sample), state);
  gst_object_unref(element);
  return sink_bin;
}

static void stop_locked(HeriPlayer *state) {
  g_atomic_int_set(&state->stop_requested, TRUE);
  if (state->bus != NULL) gst_bus_set_flushing(state->bus, TRUE);
  if (state->bus_thread != NULL) {
    g_thread_join(state->bus_thread);
    state->bus_thread = NULL;
  }
  if (state->playbin != NULL) {
    gst_element_set_state(state->playbin, GST_STATE_NULL);
    gst_object_unref(state->playbin);
    state->playbin = NULL;
  }
  if (state->bus != NULL) {
    gst_object_unref(state->bus);
    state->bus = NULL;
  }
  pthread_mutex_lock(&state->window_mutex);
  ANativeWindow *window = state->window;
  state->window = NULL;
  pthread_mutex_unlock(&state->window_mutex);
  if (window != NULL) ANativeWindow_release(window);
  g_atomic_int_set(&state->desired_playing, FALSE);
  g_atomic_int_set(&state->stop_requested, FALSE);
  g_atomic_int_set(&state->buffering_percent, 100);
  g_atomic_int_set(&state->rate_milli, 1000);
}

JNIEXPORT jboolean JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeOpen(
    JNIEnv *env, jobject instance, jstring uri, jobject surface) {
  (void)instance;
  if (uri == NULL || surface == NULL) return JNI_FALSE;
  const char *native_uri = (*env)->GetStringUTFChars(env, uri, NULL);
  if (native_uri == NULL) return JNI_FALSE;

  pthread_mutex_lock(&player.mutex);
  stop_locked(&player);
  ANativeWindow *window = ANativeWindow_fromSurface(env, surface);
  pthread_mutex_lock(&player.window_mutex);
  player.window = window;
  pthread_mutex_unlock(&player.window_mutex);
  player.playbin = gst_element_factory_make("playbin", "heriheri-player");
  GstElement *video_sink = create_android_video_sink(&player);
  if (window == NULL || player.playbin == NULL ||
      video_sink == NULL ||
      ANativeWindow_setBuffersGeometry(window, 0, 0,
                                       WINDOW_FORMAT_RGBA_8888) != 0) {
    if (video_sink != NULL) gst_object_unref(video_sink);
    stop_locked(&player);
    pthread_mutex_unlock(&player.mutex);
    (*env)->ReleaseStringUTFChars(env, uri, native_uri);
    return JNI_FALSE;
  }

  gst_object_ref_sink(video_sink);
  guint play_flags = 0;
  g_object_get(player.playbin, "flags", &play_flags, NULL);
  play_flags |= HERI_PLAY_FLAG_DOWNLOAD | HERI_PLAY_FLAG_BUFFERING;
  g_object_set(player.playbin, "uri", native_uri, "video-sink", video_sink,
               "force-aspect-ratio", TRUE,
               "flags", play_flags,
               "buffer-duration", (gint64)HERI_BUFFER_DURATION,
               "buffer-size", (gint)HERI_BUFFER_SIZE,
               "ring-buffer-max-size", (guint64)HERI_RING_BUFFER_SIZE, NULL);
  gst_object_unref(video_sink);
  player.bus = gst_element_get_bus(player.playbin);
  g_atomic_int_set(&player.desired_playing, TRUE);
  player.bus_thread =
      g_thread_new("heriheri-gstreamer-bus", run_bus, &player);
  GstElement *playbin = GST_ELEMENT(gst_object_ref(player.playbin));
  pthread_mutex_unlock(&player.mutex);
  GstStateChangeReturn result = gst_element_set_state(playbin, GST_STATE_PLAYING);
  if (result == GST_STATE_CHANGE_FAILURE) {
    pthread_mutex_lock(&player.mutex);
    if (player.playbin == playbin) stop_locked(&player);
    pthread_mutex_unlock(&player.mutex);
  }
  gst_object_unref(playbin);
  (*env)->ReleaseStringUTFChars(env, uri, native_uri);
  return result == GST_STATE_CHANGE_FAILURE ? JNI_FALSE : JNI_TRUE;
}

JNIEXPORT void JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativePlay(
    JNIEnv *env, jobject instance) {
  (void)env; (void)instance;
  pthread_mutex_lock(&player.mutex);
  g_atomic_int_set(&player.desired_playing, TRUE);
  if (player.playbin != NULL) {
    gint64 position = 0, duration = 0;
    if (gst_element_query_position(player.playbin, GST_FORMAT_TIME, &position) &&
        gst_element_query_duration(player.playbin, GST_FORMAT_TIME, &duration) &&
        position + 250 * GST_MSECOND >= duration) {
      gdouble rate = g_atomic_int_get(&player.rate_milli) / 1000.0;
      seek_at_rate(player.playbin, 0, rate);
    }
    if (g_atomic_int_get(&player.buffering_percent) >= 100)
      gst_element_set_state(player.playbin, GST_STATE_PLAYING);
  }
  pthread_mutex_unlock(&player.mutex);
}

JNIEXPORT void JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativePause(
    JNIEnv *env, jobject instance) {
  (void)env; (void)instance;
  pthread_mutex_lock(&player.mutex);
  g_atomic_int_set(&player.desired_playing, FALSE);
  if (player.playbin != NULL) gst_element_set_state(player.playbin, GST_STATE_PAUSED);
  pthread_mutex_unlock(&player.mutex);
}

JNIEXPORT void JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeStop(
    JNIEnv *env, jobject instance) {
  (void)env; (void)instance;
  pthread_mutex_lock(&player.mutex);
  stop_locked(&player);
  pthread_mutex_unlock(&player.mutex);
}

JNIEXPORT jboolean JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSeek(
    JNIEnv *env, jobject instance, jlong position_ms) {
  (void)env; (void)instance;
  pthread_mutex_lock(&player.mutex);
  gdouble rate = g_atomic_int_get(&player.rate_milli) / 1000.0;
  gboolean result = player.playbin != NULL &&
      seek_at_rate(player.playbin, (gint64)position_ms * GST_MSECOND, rate);
  pthread_mutex_unlock(&player.mutex);
  return result ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT void JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSetVolume(
    JNIEnv *env, jobject instance, jdouble volume) {
  (void)env; (void)instance;
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) g_object_set(player.playbin, "volume", volume, NULL);
  pthread_mutex_unlock(&player.mutex);
}

JNIEXPORT void JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSetMuted(
    JNIEnv *env, jobject instance, jboolean muted) {
  (void)env; (void)instance;
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) g_object_set(player.playbin, "mute", muted == JNI_TRUE, NULL);
  pthread_mutex_unlock(&player.mutex);
}

JNIEXPORT jboolean JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSetRate(
    JNIEnv *env, jobject instance, jdouble rate) {
  (void)env; (void)instance;
  pthread_mutex_lock(&player.mutex);
  gint64 position = 0;
  gboolean result = player.playbin != NULL &&
      gst_element_query_position(player.playbin, GST_FORMAT_TIME, &position) &&
      seek_at_rate(player.playbin, position, rate);
  if (result) g_atomic_int_set(&player.rate_milli, (gint)(rate * 1000.0));
  pthread_mutex_unlock(&player.mutex);
  return result ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT void JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSetLooping(
    JNIEnv *env, jobject instance, jboolean looping) {
  (void)env; (void)instance;
  g_atomic_int_set(&player.looping, looping == JNI_TRUE);
}

static jint int_property(const char *name) {
  gint value = -1;
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) g_object_get(player.playbin, name, &value, NULL);
  pthread_mutex_unlock(&player.mutex);
  return value;
}

#define INT_GETTER(method, property) \
JNIEXPORT jint JNICALL Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_##method( \
    JNIEnv *env, jobject instance) { (void)env; (void)instance; return int_property(property); }

INT_GETTER(nativeAudioTrackCount, "n-audio")
INT_GETTER(nativeSubtitleTrackCount, "n-text")
INT_GETTER(nativeCurrentAudioTrack, "current-audio")
INT_GETTER(nativeCurrentSubtitleTrack, "current-text")

static jboolean set_int_property(const char *name, jint index) {
  pthread_mutex_lock(&player.mutex);
  gboolean ok = player.playbin != NULL;
  if (ok) g_object_set(player.playbin, name, (gint)index, NULL);
  pthread_mutex_unlock(&player.mutex);
  return ok ? JNI_TRUE : JNI_FALSE;
}

JNIEXPORT jboolean JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSelectAudioTrack(
    JNIEnv *env, jobject instance, jint index) {
  (void)env; (void)instance; return set_int_property("current-audio", index);
}

JNIEXPORT jboolean JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSelectSubtitleTrack(
    JNIEnv *env, jobject instance, jint index) {
  (void)env; (void)instance; return set_int_property("current-text", index);
}

JNIEXPORT jboolean JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeSetSubtitleUri(
    JNIEnv *env, jobject instance, jstring uri) {
  (void)instance;
  const char *value = uri == NULL ? NULL : (*env)->GetStringUTFChars(env, uri, NULL);
  pthread_mutex_lock(&player.mutex);
  gboolean ok = player.playbin != NULL;
  if (ok) g_object_set(player.playbin, "suburi", value, NULL);
  pthread_mutex_unlock(&player.mutex);
  if (value != NULL) (*env)->ReleaseStringUTFChars(env, uri, value);
  return ok ? JNI_TRUE : JNI_FALSE;
}

static jlong query_time(gboolean duration) {
  gint64 value = GST_CLOCK_TIME_NONE;
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) {
    if (duration) gst_element_query_duration(player.playbin, GST_FORMAT_TIME, &value);
    else gst_element_query_position(player.playbin, GST_FORMAT_TIME, &value);
  }
  pthread_mutex_unlock(&player.mutex);
  return value == GST_CLOCK_TIME_NONE ? -1 : (jlong)(value / GST_MSECOND);
}

JNIEXPORT jlong JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativePosition(
    JNIEnv *env, jobject instance) { (void)env; (void)instance; return query_time(FALSE); }
JNIEXPORT jlong JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeDuration(
    JNIEnv *env, jobject instance) { (void)env; (void)instance; return query_time(TRUE); }
JNIEXPORT jboolean JNICALL
Java_com_heriheri_gstreamerplayer_GStreamerPlayerPlugin_nativeIsPlaying(
    JNIEnv *env, jobject instance) {
  (void)env; (void)instance;
  return g_atomic_int_get(&player.desired_playing) ? JNI_TRUE : JNI_FALSE;
}
