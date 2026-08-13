#include "HeriGStreamerPlayer.h"

#include <gst/gst.h>
#include <gst/video/videooverlay.h>
#include <pthread.h>

typedef struct {
  pthread_mutex_t mutex;
  GstElement *playbin;
  GstBus *bus;
  GThread *bus_thread;
  void *native_view;
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
  if (!gst_is_video_overlay_prepare_window_handle_message(message))
    return GST_BUS_PASS;
  if (state->native_view != NULL)
    gst_video_overlay_set_window_handle(
        GST_VIDEO_OVERLAY(GST_MESSAGE_SRC(message)),
        (guintptr)state->native_view);
  gst_message_unref(message);
  return GST_BUS_DROP;
}

static void stop_locked(void) {
  g_atomic_int_set(&player.stop_requested, TRUE);
  if (player.bus != NULL) gst_bus_set_flushing(player.bus, TRUE);
  if (player.bus_thread != NULL) {
    g_thread_join(player.bus_thread);
    player.bus_thread = NULL;
  }
  if (player.playbin != NULL) {
    gst_element_set_state(player.playbin, GST_STATE_NULL);
    gst_object_unref(player.playbin);
    player.playbin = NULL;
  }
  if (player.bus != NULL) {
    gst_bus_set_sync_handler(player.bus, NULL, NULL, NULL);
    gst_object_unref(player.bus);
    player.bus = NULL;
  }
  player.native_view = NULL;
  g_atomic_int_set(&player.desired_playing, FALSE);
  g_atomic_int_set(&player.stop_requested, FALSE);
  g_atomic_int_set(&player.rate_milli, 1000);
}

bool heri_gstreamer_open(const char *uri, void *native_view) {
  if (uri == NULL || native_view == NULL) return false;
  pthread_mutex_lock(&player.mutex);
  stop_locked();
  gst_init(NULL, NULL);
  player.native_view = native_view;
  player.playbin = gst_element_factory_make("playbin", "heriheri-player");
  GstElement *processor =
      gst_element_factory_make("identity", "heriheri-frame-processor-slot");
  if (player.playbin == NULL || processor == NULL) {
    if (processor != NULL) gst_object_unref(processor);
    stop_locked();
    pthread_mutex_unlock(&player.mutex);
    return false;
  }
  g_object_set(player.playbin, "uri", uri, "video-filter", processor,
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
  return result != GST_STATE_CHANGE_FAILURE;
}

void heri_gstreamer_play(void) {
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

void heri_gstreamer_pause(void) {
  pthread_mutex_lock(&player.mutex);
  g_atomic_int_set(&player.desired_playing, FALSE);
  if (player.playbin != NULL) gst_element_set_state(player.playbin, GST_STATE_PAUSED);
  pthread_mutex_unlock(&player.mutex);
}

void heri_gstreamer_stop(void) {
  pthread_mutex_lock(&player.mutex);
  stop_locked();
  pthread_mutex_unlock(&player.mutex);
}

bool heri_gstreamer_seek(uint64_t position_ms) {
  pthread_mutex_lock(&player.mutex);
  gdouble rate = g_atomic_int_get(&player.rate_milli) / 1000.0;
  gboolean result = player.playbin != NULL &&
      seek_at_rate(player.playbin, (gint64)position_ms * GST_MSECOND, rate);
  pthread_mutex_unlock(&player.mutex);
  return result;
}

void heri_gstreamer_set_volume(double volume) {
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) g_object_set(player.playbin, "volume", volume, NULL);
  pthread_mutex_unlock(&player.mutex);
}

void heri_gstreamer_set_muted(bool muted) {
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) g_object_set(player.playbin, "mute", muted, NULL);
  pthread_mutex_unlock(&player.mutex);
}

bool heri_gstreamer_set_rate(double rate) {
  pthread_mutex_lock(&player.mutex);
  gint64 position = 0;
  gboolean result = player.playbin != NULL &&
      gst_element_query_position(player.playbin, GST_FORMAT_TIME, &position) &&
      seek_at_rate(player.playbin, position, rate);
  if (result) g_atomic_int_set(&player.rate_milli, (gint)(rate * 1000.0));
  pthread_mutex_unlock(&player.mutex);
  return result;
}

void heri_gstreamer_set_looping(bool looping) {
  g_atomic_int_set(&player.looping, looping);
}

static int32_t int_property(const char *name) {
  gint value = -1;
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) g_object_get(player.playbin, name, &value, NULL);
  pthread_mutex_unlock(&player.mutex);
  return value;
}

int32_t heri_gstreamer_audio_track_count(void) { return int_property("n-audio"); }
int32_t heri_gstreamer_subtitle_track_count(void) { return int_property("n-text"); }
int32_t heri_gstreamer_current_audio_track(void) { return int_property("current-audio"); }
int32_t heri_gstreamer_current_subtitle_track(void) { return int_property("current-text"); }

static bool set_int_property(const char *name, int32_t value) {
  pthread_mutex_lock(&player.mutex);
  bool ok = player.playbin != NULL;
  if (ok) g_object_set(player.playbin, name, (gint)value, NULL);
  pthread_mutex_unlock(&player.mutex);
  return ok;
}

bool heri_gstreamer_select_audio_track(int32_t index) {
  return set_int_property("current-audio", index);
}
bool heri_gstreamer_select_subtitle_track(int32_t index) {
  return set_int_property("current-text", index);
}

bool heri_gstreamer_set_subtitle_uri(const char *uri) {
  pthread_mutex_lock(&player.mutex);
  bool ok = player.playbin != NULL;
  if (ok) g_object_set(player.playbin, "suburi", uri, NULL);
  pthread_mutex_unlock(&player.mutex);
  return ok;
}

static int64_t query_time(bool duration) {
  gint64 value = GST_CLOCK_TIME_NONE;
  pthread_mutex_lock(&player.mutex);
  if (player.playbin != NULL) {
    if (duration) gst_element_query_duration(player.playbin, GST_FORMAT_TIME, &value);
    else gst_element_query_position(player.playbin, GST_FORMAT_TIME, &value);
  }
  pthread_mutex_unlock(&player.mutex);
  return value == GST_CLOCK_TIME_NONE ? -1 : value / GST_MSECOND;
}

int64_t heri_gstreamer_position(void) { return query_time(false); }
int64_t heri_gstreamer_duration(void) { return query_time(true); }
bool heri_gstreamer_is_playing(void) {
  return g_atomic_int_get(&player.desired_playing);
}
