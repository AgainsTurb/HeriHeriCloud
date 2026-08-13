#ifndef HERI_GSTREAMER_PLAYER_H
#define HERI_GSTREAMER_PLAYER_H

#include <stdbool.h>
#include <stdint.h>

bool heri_gstreamer_open(const char *uri, void *native_view);
void heri_gstreamer_play(void);
void heri_gstreamer_pause(void);
void heri_gstreamer_stop(void);
bool heri_gstreamer_seek(uint64_t position_ms);
void heri_gstreamer_set_volume(double volume);
void heri_gstreamer_set_muted(bool muted);
bool heri_gstreamer_set_rate(double rate);
void heri_gstreamer_set_looping(bool looping);
int32_t heri_gstreamer_audio_track_count(void);
int32_t heri_gstreamer_subtitle_track_count(void);
int32_t heri_gstreamer_current_audio_track(void);
int32_t heri_gstreamer_current_subtitle_track(void);
bool heri_gstreamer_select_audio_track(int32_t index);
bool heri_gstreamer_select_subtitle_track(int32_t index);
bool heri_gstreamer_set_subtitle_uri(const char *uri);
int64_t heri_gstreamer_position(void);
int64_t heri_gstreamer_duration(void);
bool heri_gstreamer_is_playing(void);

#endif
