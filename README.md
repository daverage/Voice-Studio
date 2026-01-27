# Voice Studio (VxCleaner)

Professional vocal restoration and enhancement suite for spoken voice.

## Overview

Voice Studio is a professional vocal restoration and enhancement suite built for podcasting, voice-over, dialogue, and broadcast material.

It focuses on improving clarity, consistency, and intelligibility without turning natural speech into something processed or artificial. The goal isn't just noise removal—it's making voice easier to listen to, easier to mix, and ready for delivery across platforms.

## Architecture

Voice Studio uses a hybrid approach, combining traditional high-precision DSP with neural network (DTLN) processing for broadband noise suppression. The DTLN engine is a core part of the plugin and is optimized for CPU inference to ensure maximum stability and cross-platform compatibility.

### DSP Pipeline

1. **Hygiene**: 90Hz High-Pass Filter (`SpeechHpf`)
2. **Analysis**: Speech Confidence Sidechain
3. **Early Reflection**: Micro-deverb for desk/wall coloration
4. **Denoiser**: Hybrid Spectral + Neural suppression
5. **Plosive Softener**: Automatic thump protection
6. **Breath Reducer**: Confidence-weighted breath softening
7. **Deverber**: Late reverb tail reduction
8. **Shaping**: Proximity (body) and Clarity (air)
9. **Dynamics**: De-esser, Leveler, and Limiter

## Features

- **Simple Mode**: Intent-based macro controls (Distance, Clarity, Consistency) for rapid results.
- **Advanced Mode**: Granular access to every stage of the restoration and dynamics chain.
- **Neural Denoising**: Embedded DTLN models for professional-grade noise suppression.
- **Global Reset**: Instantly clear all DSP state and return to defaults.
- **Delivery Presets**: Built-in loudness targeting for YouTube, Spotify, and Broadcast standards.

## Building

### Prerequisites
- Rust 1.70+
- `cargo-nih-plug` (install with `cargo install cargo-nih-plug`)

### macOS TensorFlow Lite setup
The crate now links against a prebuilt TensorFlow Lite library, so builds skip `bindgen` and the makefile entirely. Before running `cargo` you must point the build script at a native `.a` (or `.dylib`) by exporting:

```
export TFLITE_LIB_DIR=/path/to/libtensorflow-lite.a
```

If you have an architecture-specific build (e.g., `aarch64-apple-darwin`), you can also set `TFLITE_AARCH64_APPLE_DARWIN_LIB_DIR`. After that any of the normal commands will link that binary instead of rebuilding TFLite.

### Commands

```bash
# Bundle VST3/CLAP plugins (Production)
cargo nih-plug bundle vxcleaner --release

# Bundle with Debug features (Enables logging and UI Log button)
cargo nih-plug bundle vxcleaner --release --features debug

# Run tests
cargo test
```

## Feature Flags

- **debug**: Enables centralized logging to `/tmp/voice_studio.log` and shows the "Log" button in the plugin footer.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with the [nih-plug](https://github.com/robbert-vdh/nih-plug) framework.
- Neural inference powered by [tract](https://github.com/snipsco/tract).
- UI built with [vizia](https://github.com/vizia/vizia).
