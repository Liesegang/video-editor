#!/bin/sh
set -eu

fixture_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
media_ffmpeg=${MEDIA_FIXTURE_FFMPEG:-ffmpeg}
media_ffprobe=${MEDIA_FIXTURE_FFPROBE:-ffprobe}
media_cwebp=${MEDIA_FIXTURE_CWEBP:-cwebp}

if ! command -v "$media_ffmpeg" >/dev/null 2>&1; then
    echo "FFmpeg is required: $media_ffmpeg" >&2
    exit 1
fi
if ! command -v "$media_ffprobe" >/dev/null 2>&1; then
    echo "ffprobe is required: $media_ffprobe" >&2
    exit 1
fi
if ! command -v "$media_cwebp" >/dev/null 2>&1; then
    echo "cwebp is required: $media_cwebp" >&2
    exit 1
fi
if ! command -v shasum >/dev/null 2>&1; then
    echo "shasum is required to update SHA256SUMS" >&2
    exit 1
fi

ffmpeg_run() {
    "$media_ffmpeg" -hide_banner -loglevel error -y "$@"
}

# Static images: alpha, lossy RGB, and lossless alpha WebP.
ffmpeg_run -f lavfi \
    -i "nullsrc=size=8x6,format=rgba,geq=r='X*31':g='Y*43':b='(X+Y)*19':a='if(lt(X,4),255,80)'" \
    -frames:v 1 -c:v png -pix_fmt rgba "$fixture_dir/rgba.png"
ffmpeg_run -f lavfi -i "testsrc=size=8x6:rate=1" \
    -frames:v 1 -c:v mjpeg -q:v 2 -pix_fmt yuvj444p "$fixture_dir/rgb.jpg"
"$media_cwebp" -quiet -lossless "$fixture_dir/rgba.png" \
    -o "$fixture_dir/rgba.webp"

# Non-30fps H.264 source. MOV is a remux of the same elementary stream so
# decoded frame equality also checks container-independent loading.
ffmpeg_run -f lavfi \
    -i "nullsrc=size=12x8:rate=24:duration=3,geq=lum='mod(X*17+Y*13+N*7,256)':cb='mod(64+N*5,256)':cr='mod(192-N*3,256)'" \
    -an -c:v libx264 -preset medium -threads 1 -g 12 -keyint_min 12 \
    -sc_threshold 0 -pix_fmt yuv420p -movflags +faststart "$fixture_dir/h264_24.mp4"
ffmpeg_run -i "$fixture_dir/h264_24.mp4" -map 0:v:0 -an -c copy \
    "$fixture_dir/h264_24.mov"

# VP9 retains an odd-sized 9x7 frame, exercising stride and dimension logic.
ffmpeg_run -f lavfi -i "testsrc=size=9x7:rate=15:duration=2" \
    -an -c:v libvpx-vp9 -lossless 1 -threads 1 -row-mt 0 -g 10 \
    -pix_fmt yuv444p "$fixture_dir/vp9_odd.webm"

# FFV1 preserves alpha in an odd-sized Matroska source.
ffmpeg_run -f lavfi \
    -i "nullsrc=size=7x5:rate=12:duration=1,format=rgba,geq=r='X*35':g='Y*50':b='N*17':a='if(lt(X,3),255,64)'" \
    -an -c:v ffv1 -level 3 -pix_fmt bgra "$fixture_dir/ffv1_alpha.mkv"

# Two independently selectable video streams with visibly different colors.
ffmpeg_run \
    -f lavfi -i "color=color=red:size=8x6:rate=12:duration=1" \
    -f lavfi -i "color=color=blue:size=8x6:rate=12:duration=1" \
    -map 0:v:0 -map 1:v:0 -an -c:v ffv1 -level 3 \
    "$fixture_dir/multistream.mkv"

# Four frames with deliberately irregular display PTS (0.0, 0.1, 0.5, 1.8)
# while the advertised average/rate remains 10 fps. An ordinal implementation
# selects the future 1.8s frame at t=1.0; a PTS implementation holds 0.5s.
ffmpeg_run -f lavfi \
    -i "nullsrc=size=8x6:rate=10:duration=0.4,geq=r='N*60':g='255-N*50':b='N*35'" \
    -vf "setpts='if(eq(N,0),0,if(eq(N,1),0.1,if(eq(N,2),0.5,1.8)))/TB'" \
    -fps_mode vfr -an -c:v ffv1 -level 3 -pix_fmt bgr0 \
    "$fixture_dir/vfr_pts.mkv"

vfr_pts=$(
    "$media_ffprobe" -v error -select_streams v:0 \
        -show_entries frame=best_effort_timestamp_time \
        -of csv=p=0 "$fixture_dir/vfr_pts.mkv"
)
expected_vfr_pts=$(printf '0.000000\n0.100000\n0.500000\n1.800000')
if [ "$vfr_pts" != "$expected_vfr_pts" ]; then
    echo "unexpected VFR frame PTS:" >&2
    printf '%s\n' "$vfr_pts" >&2
    exit 1
fi
vfr_rate=$(
    "$media_ffprobe" -v error -select_streams v:0 \
        -show_entries stream=avg_frame_rate -of default=nw=1:nk=1 \
        "$fixture_dir/vfr_pts.mkv"
)
if [ "$vfr_rate" != "10/1" ]; then
    echo "unexpected VFR advertised average rate: $vfr_rate" >&2
    exit 1
fi

# A tiny audio-only fixture documents the FFmpeg metadata path. Audio samples
# are mixed by the dedicated audio loader rather than the image graph.
ffmpeg_run -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1" \
    -vn -c:a libmp3lame -b:a 64k "$fixture_dir/tone.mp3"

# Global stream 0 is video. Audio stream 1 is a 440 Hz left-only signal and
# global stream 2 is an 880 Hz right-only signal. This verifies that persisted
# FFmpeg stream ordinals are resolved before Symphonia Track IDs are used.
ffmpeg_run \
    -f lavfi -i "color=color=black:size=8x6:rate=12:duration=3" \
    -f lavfi -i "sine=frequency=440:sample_rate=8000:duration=3" \
    -f lavfi -i "sine=frequency=880:sample_rate=8000:duration=3" \
    -filter_complex "[1:a]pan=stereo|c0=c0|c1=0*c0[left];[2:a]pan=stereo|c0=0*c0|c1=c0[right]" \
    -map 0:v:0 -map "[left]" -map "[right]" \
    -c:v ffv1 -level 3 -c:a pcm_s16le "$fixture_dir/multi_audio.mkv"

# Embedded audio plus deliberately mismatched stream durations: the video has
# exactly 12 frames/1 second while the audio and container continue for 2
# seconds. This catches code that substitutes duration for the authoritative
# half-open video frame bound.
ffmpeg_run \
    -f lavfi -i "testsrc2=size=12x8:rate=12:duration=1" \
    -f lavfi -i "sine=frequency=660:sample_rate=48000:duration=2" \
    -map 0:v:0 -map 1:a:0 \
    -c:v libx264 -preset medium -threads 1 -g 6 -keyint_min 6 \
    -sc_threshold 0 -pix_fmt yuv420p \
    -c:a aac -b:a 64k -movflags +faststart \
    "$fixture_dir/av_duration_mismatch.mp4"

(
    cd "$fixture_dir"
    shasum -a 256 \
        rgba.png rgb.jpg rgba.webp h264_24.mp4 h264_24.mov \
        vp9_odd.webm ffv1_alpha.mkv multistream.mkv tone.mp3 multi_audio.mkv \
        av_duration_mismatch.mp4 vfr_pts.mkv \
        > SHA256SUMS
)
