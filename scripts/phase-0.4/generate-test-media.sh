#!/usr/bin/env bash
# ==============================================================================
# Muvrix Phase 0.4 — Test Media Generator
# Generates concise, representative test fixtures using local FFmpeg encoders.
# Adheres strictly to the Muvrix Phase 0.4 Specification:
# - Checks encoder availability before attempting generation.
# - Never overwrites existing user media without explicit --force.
# - Generates short (5-second) controlled fixtures.
# - Reports clear status and failure reasons.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/../.." && pwd)"
OUTPUT_DIR="${1:-${ROOT_DIR}/tests/MuvrixTestMedia/phase-0.4/generated}"
FORCE_OVERWRITE=false

for arg in "$@"; do
    if [[ "$arg" == "--force" ]]; then
        FORCE_OVERWRITE=true
    fi
done

echo "========================================================================"
echo " Muvrix Phase 0.4 Test Media Generator"
echo " Target output directory: ${OUTPUT_DIR}"
echo " Overwrite existing:      ${FORCE_OVERWRITE}"
echo "========================================================================"

mkdir -p "${OUTPUT_DIR}"

# 1. Verify FFmpeg installation
if ! command -v ffmpeg &> /dev/null; then
    echo "ERROR: ffmpeg is not installed or not in PATH."
    exit 1
fi

FFMPEG_ENCODERS=$(ffmpeg -encoders 2>&1)

has_encoder() {
    local enc="$1"
    echo "${FFMPEG_ENCODERS}" | grep -qE "^\s*V\S*\s+${enc}\s+"
}

has_audio_encoder() {
    local enc="$1"
    echo "${FFMPEG_ENCODERS}" | grep -qE "^\s*A\S*\s+${enc}\s+"
}

echo "Detected Encoder Capabilities:"
echo " - libx264:    $(has_encoder libx264 && echo 'YES' || echo 'NO')"
echo " - libx265:    $(has_encoder libx265 && echo 'YES' || echo 'NO')"
echo " - libvpx-vp9: $(has_encoder libvpx-vp9 && echo 'YES' || echo 'NO')"
echo " - libsvtav1:  $(has_encoder libsvtav1 && echo 'YES' || echo 'NO')"
echo " - aac:        $(has_audio_encoder aac && echo 'YES' || echo 'NO')"
echo "------------------------------------------------------------------------"

generate_fixture() {
    local target="$1"
    shift
    local desc="$1"
    shift

    local out_path="${OUTPUT_DIR}/${target}"

    if [[ -f "${out_path}" ]] && [[ "${FORCE_OVERWRITE}" != "true" ]]; then
        echo "[SKIP] ${target} (already exists; use --force to overwrite)"
        return 0
    fi

    echo "[GENERATING] ${target} (${desc})..."
    if ffmpeg -v error -y "$@" "${out_path}"; then
        local size
        size=$(stat -c%s "${out_path}" 2>/dev/null || stat -f%z "${out_path}")
        echo "  --> SUCCESS: ${target} (${size} bytes)"
    else
        echo "  --> FAILED: ${target}"
        rm -f "${out_path}"
        return 1
    fi
}

# ------------------------------------------------------------------------------
# 1. H.264 1080p 60 FPS
# ------------------------------------------------------------------------------
if has_encoder libx264; then
    generate_fixture "h264-1080p-60fps.mp4" "H.264 High Profile, 1080p, 60 FPS, AAC audio" \
        -f lavfi -i "testsrc=duration=5:size=1920x1080:rate=60" \
        -f lavfi -i "sine=frequency=440:duration=5" \
        -c:v libx264 -pix_fmt yuv420p -preset fast -crf 20 \
        -c:a aac -b:a 128k
else
    echo "[UNSUPPORTED] libx264 not available; skipping H.264 60fps fixture"
fi

# ------------------------------------------------------------------------------
# 2. H.264 1080p 120 FPS
# ------------------------------------------------------------------------------
if has_encoder libx264; then
    generate_fixture "h264-1080p-120fps.mp4" "H.264 High Profile, 1080p, 120 FPS high-frame-rate" \
        -f lavfi -i "testsrc=duration=4:size=1920x1080:rate=120" \
        -c:v libx264 -pix_fmt yuv420p -preset fast -crf 22 -an
else
    echo "[UNSUPPORTED] libx264 not available; skipping H.264 120fps fixture"
fi

# ------------------------------------------------------------------------------
# 3. H.264 4K 60 FPS
# ------------------------------------------------------------------------------
if has_encoder libx264; then
    generate_fixture "h264-4k-60fps.mp4" "H.264 3840x2160 UHD, 60 FPS performance test" \
        -f lavfi -i "testsrc=duration=5:size=3840x2160:rate=60" \
        -c:v libx264 -pix_fmt yuv420p -preset ultrafast -crf 24 -an
else
    echo "[UNSUPPORTED] libx264 not available; skipping 4K 60fps fixture"
fi

# ------------------------------------------------------------------------------
# 4. HEVC Main10 HDR10 / PQ (SMPTE ST 2084) Synthetic Pattern
# ------------------------------------------------------------------------------
if has_encoder libx265; then
    generate_fixture "hevc-main10-hdr10-synthetic.mp4" "HEVC Main10 10-bit HDR10 BT.2020 SMPTE-2084 PQ" \
        -f lavfi -i "smptebars=duration=5:size=1920x1080:rate=30" \
        -c:v libx265 -pix_fmt yuv420p10le -preset fast -crf 22 \
        -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc \
        -x265-params "colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400" \
        -an
else
    echo "[UNSUPPORTED] libx265 not available; skipping HEVC Main10 HDR10 fixture"
fi

# ------------------------------------------------------------------------------
# 5. HEVC Main10 HLG (ARIB STD-B67) Synthetic Pattern
# ------------------------------------------------------------------------------
if has_encoder libx265; then
    generate_fixture "hevc-main10-hlg-synthetic.mp4" "HEVC Main10 10-bit HLG BT.2020 ARIB STD-B67" \
        -f lavfi -i "testsrc2=duration=5:size=1920x1080:rate=30" \
        -c:v libx265 -pix_fmt yuv420p10le -preset fast -crf 22 \
        -color_primaries bt2020 -color_trc arib-std-b67 -colorspace bt2020nc \
        -x265-params "colorprim=bt2020:transfer=arib-std-b67:colormatrix=bt2020nc" \
        -an
else
    echo "[UNSUPPORTED] libx265 not available; skipping HEVC Main10 HLG fixture"
fi

# ------------------------------------------------------------------------------
# 6. VP9 Profile 2 (10-bit 4:2:0)
# ------------------------------------------------------------------------------
if has_encoder libvpx-vp9; then
    generate_fixture "vp9-profile2-10bit-synthetic.webm" "VP9 Profile 2 10-bit yuv420p10le" \
        -f lavfi -i "testsrc=duration=5:size=1280x720:rate=30" \
        -c:v libvpx-vp9 -profile:v 2 -pix_fmt yuv420p10le -b:v 2M -an
else
    echo "[UNSUPPORTED] libvpx-vp9 not available; skipping VP9 Profile 2 fixture"
fi

# ------------------------------------------------------------------------------
# 7. AV1 Main Profile 1080p
# ------------------------------------------------------------------------------
if has_encoder libsvtav1; then
    generate_fixture "av1-1080p-svtav1-synthetic.webm" "AV1 Main Profile 1080p 30 FPS" \
        -f lavfi -i "testsrc=duration=4:size=1920x1080:rate=30" \
        -c:v libsvtav1 -pix_fmt yuv420p -preset 8 -crf 30 -an
elif has_encoder libaom-av1; then
    generate_fixture "av1-1080p-aom-synthetic.webm" "AV1 Main Profile 1080p 30 FPS via libaom" \
        -f lavfi -i "testsrc=duration=3:size=1280x720:rate=30" \
        -c:v libaom-av1 -pix_fmt yuv420p -crf 30 -an
else
    echo "[UNSUPPORTED] No AV1 encoder (libsvtav1 or libaom-av1) found; skipping AV1 fixture"
fi

echo "========================================================================"
echo " Media generation complete. Fixtures stored in ${OUTPUT_DIR}"
echo "========================================================================"
