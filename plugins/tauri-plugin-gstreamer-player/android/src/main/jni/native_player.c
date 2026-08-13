#include <android/native_window.h>
#include <android/native_window_jni.h>
#include <gst/gst.h>
#include <gst/video/videooverlay.h>
#include <jni.h>
#include <pthread.h>
#include <stdint.h>

typedef struct {
  pthread_mutex_t mutex;
  GstElement *playbin;
  GstBus *bus;
  GThread *bus_thread;
  ANativeWindow *window;
  gint stop_requested;
  gint desired_playing;
  gint looping;
  gint rate_milli;
} HeriPlayer;

static HeriPlayer player = {
    .mutex = PTHREAD_MUTEX_INITIALIZER,
    .rate_milli = 1000,
};

static gboolean seek_at_rate(GstElement *playbin, gint64 position,
                             gdouble rate) {
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
        GST_MESSAGE_EOS | GST_MESSAGE_ERROR);
    if (message == NULL) continue;
    if (GST_MESSAGE_TYPE(message) == GST_MESSAGE_EOS &&
        g_atomic_int_get(&state->looping)) {
      gdouble rate = g_atomic_int_get(&state->rate_milli) / 1000.0;
      seek_at_rate(state->playbin, 0, rate);
      if (g_atomic_int_get(&state->desired_playing))
        gst_element_set_state(state->playbin, GST_STATE_PLAYING);
    } else {
      g_atomic_int_set(&state->desired_playing, FALSE);
    }
    gst_message_unref(message);
  }
  return NULL;
}

static GstBusSyncReply handle_sync_message(GstBus *bus, GstMessage *message,
                                           gpointer data) {
  (void)bus;
  HeriPlayer *state = (HeriPlayer *)data;
  if (!gst_is_video_overlay_prepare_window_handle_message(message)) {
    return GST_BUS_PASS;
  }
  if (state->window != NULL) {
    gst_video_overlay_set_window_handle(
        GST_VIDEO_OVERLAY(GST_MESSAGE_SRC(message)),
        (guintptr)state->window);
  }
  gst_message_unref(message);
  return GST_BUS_DROP;
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
    gst_bus_set_sync_handler(state->bus, NULL, NULL, NULL);
    gst_object_unref(state->bus);
    state->bus = NULL;
  }
  if (state->window != NULL) {
    ANativeWindow_release(state->window);
    state->window = NULL;
  }
  g_atomic_int_set(&state->desired_playing, FALSE);
  g_atomic_int_set(&state->stop_requested, FALSE);
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
  player.window = ANativeWindow_fromSurface(env, surface);
  player.playbin = gst_element_factory_make("playbin", "heriheri-player");
  GstElement *processor =
      gst_element_factory_make("identity", "heriheri-frame-processor-slot");
  if (player.window == NULL || player.playbin == NULL ||
      processor == NULL) {
    if (processor != NULL) gst_object_unref(processor);
    stop_locked(&player);
    pthread_mutex_unlock(&player.mutex);
    (*env)->ReleaseStringUTFChars(env, uri, native_uri);
    return JNI_FALSE;
  }

  g_object_set(player.playbin, "uri", native_uri, "video-filter", processor,
               "force-aspect-ratio", TRUE, NULL);
  gst_object_unref(processor);
  player.bus = gst_element_get_bus(player.playbin);
  gst_bus_set_sync_handler(player.bus, handle_sync_message, &player, NULL);
  g_atomic_int_set(&player.desired_playing, TRUE);
  player.bus_thread =
      g_thread_new("heriheri-gstreamer-bus", run_bus, &player);
  GstElement *playbin = GST_ELEMENT(gst_object_ref(player.playbin));
  pthread_mutex_unlock(&player.mutex);
  GstStateChangeReturn result = gst_element_set_state(playbin, GST_STATE_PLAYING);
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
