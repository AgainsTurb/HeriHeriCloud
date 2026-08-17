LOCAL_PATH := $(call my-dir)

ifeq ($(strip $(GSTREAMER_ROOT_ANDROID)),)
$(error GSTREAMER_ROOT_ANDROID or the HERI_GSTREAMER_ANDROID_ROOT Gradle property must point to the extracted GStreamer Android universal SDK)
endif

include $(CLEAR_VARS)

LOCAL_MODULE := heri_gstreamer_player
LOCAL_SRC_FILES := native_player.c
LOCAL_LDLIBS := -llog -landroid
LOCAL_SHARED_LIBRARIES := gstreamer_android

include $(BUILD_SHARED_LIBRARY)

ifeq ($(TARGET_ARCH_ABI),arm64-v8a)
GSTREAMER_ROOT := $(GSTREAMER_ROOT_ANDROID)/arm64
else ifeq ($(TARGET_ARCH_ABI),armeabi-v7a)
GSTREAMER_ROOT := $(GSTREAMER_ROOT_ANDROID)/armv7
else ifeq ($(TARGET_ARCH_ABI),x86)
GSTREAMER_ROOT := $(GSTREAMER_ROOT_ANDROID)/x86
else ifeq ($(TARGET_ARCH_ABI),x86_64)
GSTREAMER_ROOT := $(GSTREAMER_ROOT_ANDROID)/x86_64
else
$(error Unsupported Android ABI $(TARGET_ARCH_ABI))
endif
GSTREAMER_NDK_BUILD_PATH := $(GSTREAMER_ROOT)/share/gst-android/ndk-build
include $(GSTREAMER_NDK_BUILD_PATH)/plugins.mk
GSTREAMER_PLUGINS := coreelements app audioconvert audioresample gio overlaycomposition pango \
    typefindfunctions deinterlace videoconvertscale videorate volume autodetect playback \
    subparse ogg theora vorbis opus adaptivedemux2 audioparsers auparse avi flac flv \
    id3demux isomp4 jpeg matroska mpg123 wavparse dash hls opusparse videoparsersbad \
    androidmedia asf mpegpsdemux mpegtsdemux libav tcp rtsp rtp rtpmanager soup udp opensles
GSTREAMER_EXTRA_DEPS := gstreamer-video-1.0
override GSTREAMER_BUILD_DIR := $(abspath $(LOCAL_PATH)/../../../build/gst-android-build/$(TARGET_ARCH_ABI))
override GSTREAMER_JAVA_SRC_DIR := $(GSTREAMER_BUILD_DIR)/java
GSTREAMER_INCLUDE_FONTS := no
GSTREAMER_INCLUDE_CA_CERTIFICATES := no
include $(GSTREAMER_NDK_BUILD_PATH)/gstreamer-1.0.mk
