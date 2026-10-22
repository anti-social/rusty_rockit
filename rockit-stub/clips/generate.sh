#!/bin/sh
# Generates the clips embedded into the fake VENC (feature `clips`).
#
# Requirements for looping playback: every clip starts with an IDR, has a closed
# GOP of 30 frames without B-frames (decode order == display order) and repeats
# the parameter sets before every IDR. Encoder info SEIs are dropped.
set -eu
cd "$(dirname "$0")"

FRAMES=60
GOP=30

for size in 1280x720 1920x1080; do
    case $size in
        1280x720) name=720p; bitrate=1M ;;
        1920x1080) name=1080p; bitrate=2M ;;
    esac
    src="testsrc2=size=$size:rate=30"

    ffmpeg -y -hide_banner -loglevel error -f lavfi -i "$src" -frames:v $FRAMES \
        -pix_fmt yuv420p -c:v libx264 -profile:v main -b:v $bitrate \
        -x264-params "keyint=$GOP:min-keyint=$GOP:scenecut=0:bframes=0:repeat-headers=1" \
        -bsf:v filter_units=remove_types=6 -f h264 "$name.h264"

    ffmpeg -y -hide_banner -loglevel error -f lavfi -i "$src" -frames:v $FRAMES \
        -pix_fmt yuv420p -c:v libx265 -profile:v main -b:v $bitrate \
        -x265-params "log-level=error:keyint=$GOP:min-keyint=$GOP:scenecut=0:bframes=0:repeat-headers=1:info=0:open-gop=0" \
        -f hevc "$name.hevc"
done
