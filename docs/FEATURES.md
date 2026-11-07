# Muvrix Features and Capabilities

This document provides a concise overview of the features currently implemented in Muvrix, features existing in the architecture, and capabilities planned for future releases.

---

## 1. Implemented Features (Phase 0.6)

### Media Playback & Controls
- **Native File Dialog**: Cross-platform open-file picker (`rfd`) with cancellation safety and support for standard video/audio extensions (`Ctrl+O`).
- **Transport Controls**: Dedicated transport bar featuring prominent Play/Pause toggle, Stop, -5s seek backward, and +5s seek forward.
- **Dedicated Timeline Row**: Full-width interactive seekbar with monospace elapsed time and total duration timestamps, decoupled drag-scrubbing preview, and hover timestamp tooltips.
- **Volume & Mute**: Fine-grained 0..100% volume slider with live percentage label, state-aware vector speaker icon toggle (`M` hotkey), and Up/Down arrow key increments.
- **Playback Speed Control**: Speed selector dropdown with standard presets (`0.25x` to `2.0x`), quick `[` and `]` key adjustments, and an instant `1x` reset button.
- **Fullscreen Presentation**: One-key fullscreen toggle (`F` or `F11`), viewport double-click toggle, and `Esc` exit.
- **Viewport Interaction**: Single-click on video viewport toggles Play/Pause; double-click toggles Fullscreen.

### Audio & Subtitle Track Management
- **Audio Stream Switching**: In-place dropdown listing all embedded audio tracks with demuxer stream IDs, language tags, titles, and codec names.
- **Subtitle Track Switching**: In-place dropdown listing all embedded subtitle tracks with language and format badges.
- **Subtitle Disablement**: Explicit "Off (Disabled)" selection to cleanly turn off subtitle rendering.
- **External Subtitle Loading**: "Add Subtitle File..." dialog for loading external `.srt`, `.ass`, `.ssa`, `.vtt`, and `.sub` files into the active session.
- **Subtitle Timing & Visibility**: On-the-fly +/-0.25s subtitle delay offset tuning and visibility toggle.

### Navigation & User Interface
- **Chapter Navigation**: Previous/Next chapter skip buttons and chapter selector dropdown showing chapter numbers, titles, and timestamps (automatically hidden when media lacks chapters).
- **Interactive UI Overlays**:
  - *Idle State*: Centered dark welcome card with primary "Open Media File" action and supported format badges.
  - *Loading / Buffering*: Semi-transparent overlay with procedural animated spinner.
  - *Error State*: High-contrast diagnostic banner with error details and an instant "Open Another File..." recovery button.
  - *Playback Ended*: Overlay offering instant "Replay" and "Open File" actions.
- **Media Information Badge**: Real-time header badge displaying resolution, video codec, audio codec, and active hardware decoder (e.g. `640×360 · H.264 · AAC · HW: vaapi-copy`).
- **Developer & Test Presets Menu**: Consolidated dropdown organizing diagnostic overlays and test media presets away from the primary user interface.
- **Keyboard Shortcuts Modal**: Quick-reference modal (`F1` or toolbar button) documenting all keyboard shortcuts, strictly guarded against text-input focus interception.
- **Procedural Vector Icons**: Custom `egui::Painter` vector geometry providing consistent, crisp icons across Linux, macOS, and Windows without emoji font discrepancies or tofu boxes.

---

## 2. Existing Architectural Capabilities (Reused)

- **Window Resizing & Aspect Ratio Letterboxing**: The OpenGL 3.3+ FBO quad compositor preserves source aspect ratio across arbitrary window dimensions and resizing events without distortion.

---

## 3. Partially Implemented Features

- **Network Stream Opening**: Basic HTTP and HTTPS streaming URLs can be opened via the path resolver; a dedicated streaming manager with playlist bookmarks is deferred to Phase 0.8.

---

## 4. Planned Features

### Phase 0.7 — Advanced Playback & Controls
- Secondary Subtitles (dual simultaneous subtitles for language learning).
- Picture-in-Picture (PiP) detached floating window.
- Playback resume & watch history persistence.
- Subtitle styling customizer (font selection, size, text/border colors).
- Playlist queue management sidebar.

### Phase 0.8 — Shaders, Audio DSP & Media Library
- Custom GLSL post-processing shaders and Anime4K upscaling presets.
- Parametric audio equalizer and dynamic range compression.
- SQLite local media library with background directory scanning and metadata extraction.
- Dedicated streaming hub (HLS, DASH, auth headers, network cache configuration).
