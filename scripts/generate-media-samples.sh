#!/usr/bin/env bash
#
# generate-media-samples.sh — produce the 4K120 test assets used by the
# v0.16.0 media gate.
#
# Outputs (file names are contractual — do not change):
#   h264-4k120.bin  60s H.264 Annex-B elementary stream (VideoToolbox, 60 Mb/s)
#   hevc-4k120.bin  60s HEVC  Annex-B elementary stream (VideoToolbox, 40 Mb/s)
#   av1-4k120.bin   15s AV1   IVF container            (libsvtav1, software)
#
# Output directory is taken from $MARTENSITE_MEDIA_SAMPLES, defaulting to
# target/media-samples relative to the repository root (gitignored build
# output — the samples are hundreds of MB and must not be committed).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${MARTENSITE_MEDIA_SAMPLES:-$REPO_ROOT/target/media-samples}"
mkdir -p "$OUT_DIR"

SRC="testsrc2=size=3840x2160:rate=120"

report() {
    local f="$1"
    ls -lh "$f"
    ffprobe -v error -count_packets \
        -show_entries stream=width,height,r_frame_rate,nb_read_packets \
        -show_entries format=duration \
        -of default=noprint_wrappers=1 "$f" || true
    echo
}

echo "==> Output directory: $OUT_DIR"

# H.264 — hardware VideoToolbox encoder, 60 s @ 3840x2160 120 fps.
# The h264_metadata bitstream filter injects VUI timing (tick_rate=120) so
# ffprobe reports r_frame_rate=120/1 on the containerless Annex-B stream;
# VideoToolbox otherwise writes no timing info and probing falls back to
# the 1/1200000 time base.
echo "==> Encoding h264-4k120.bin (h264_videotoolbox, 60M, 60s) ..."
ffmpeg -hide_banner -y \
    -f lavfi -i "$SRC" \
    -t 60 -c:v h264_videotoolbox -b:v 60M \
    -bsf:v h264_metadata=tick_rate=120 \
    -f h264 "$OUT_DIR/h264-4k120.bin"
report "$OUT_DIR/h264-4k120.bin"

# HEVC — hardware VideoToolbox encoder, 60 s @ 3840x2160 120 fps.
# Same VUI timing injection as H.264 (hevc_metadata tick_rate=120).
echo "==> Encoding hevc-4k120.bin (hevc_videotoolbox, 40M, 60s) ..."
ffmpeg -hide_banner -y \
    -f lavfi -i "$SRC" \
    -t 60 -c:v hevc_videotoolbox -b:v 40M \
    -bsf:v hevc_metadata=tick_rate=120 \
    -f hevc "$OUT_DIR/hevc-4k120.bin"
report "$OUT_DIR/hevc-4k120.bin"

# AV1 — libsvtav1 is a *software* encoder (unlike the VideoToolbox encoders
# above), so a full 60 s at 4K120 would take far too long. The sample is
# capped at 15 s (~1800 frames); that is still plenty for decoder-gate
# coverage. IVF is used so frames are self-delimiting.
echo "==> Encoding av1-4k120.bin (libsvtav1, preset 12, crf 40, 15s) ..."
ffmpeg -hide_banner -y \
    -f lavfi -i "$SRC" \
    -t 15 -c:v libsvtav1 -preset 12 -crf 40 \
    -f ivf "$OUT_DIR/av1-4k120.bin"
report "$OUT_DIR/av1-4k120.bin"

echo "==> Done. Samples written to $OUT_DIR"
