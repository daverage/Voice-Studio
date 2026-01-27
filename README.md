# Voice Studio (VxCleaner)

Professional vocal restoration and enhancement suite for spoken voice.

## Overview

Voice Studio is a professional vocal restoration and enhancement suite built for podcasting, voice-over, dialogue, and broadcast material.

It focuses on improving clarity, consistency, and intelligibility without turning natural speech into something processed or artificial. The goal isn't just noise removal—it's making voice easier to listen to, easier to mix, and ready for delivery across platforms.

## Architecture

Voice Studio is built around a tightly integrated DSP restoration pipeline that emphasizes deterministic behavior, low latency, and professional speech restoration without relying on neural models.

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
- **Global Reset**: Instantly clear all DSP state and return to defaults.
- **Delivery Presets**: Built-in loudness targeting for YouTube, Spotify, and Broadcast standards.

## Building

### Prerequisites
- Rust 1.70+
- `cargo-nih-plug` (install with `cargo install cargo-nih-plug`)

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
- UI built with [vizia](https://github.com/vizia/vizia).


## Setting Up Agents for MCP Usage

When using tinyMem as an MCP server for AI agents, ensure that your agents follow the MANDATORY TINYMEM CONTROL PROTOCOL.

Include the contract content from [AGENT_CONTRACT.md](AGENT_CONTRACT.md) in your agent's system prompt to ensure proper interaction with tinyMem.
