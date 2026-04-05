# Audio/Video Transcription

This module provides audio transcription using Whisper and video parsing with audio extraction.

## Features

- **Audio Transcription**: Convert speech to text using Whisper
- **Video Parsing**: Extract audio from video files for transcription
- **Multi-format Support**: MP3, WAV, M4A, OGG, FLAC, WebM, MP4, MOV, AVI, MKV
- **Language Detection**: Auto-detect or specify language
- **Segment-level Timestamps**: Get timestamps for each transcribed segment

## Setup

### 1. Enable Whisper Feature

In `Cargo.toml`:

```toml
[dependencies]
cogkos-ingest = { path = "...", features = ["whisper"] }
```

### 2. Download Whisper Model

Download a Whisper model from: https://huggingface.co/ggerganov/whisper.cpp/tree/master

Recommended models:
- `tiny` - Fastest, lowest accuracy (~75MB)
- `base` - Good balance (~140MB)  
- `small` - Better accuracy (~490MB)
- `medium` - High accuracy (~1.5GB)
- `large` - Best accuracy (~3GB)

Place the model file in one of these locations:
- `/usr/local/share/whisper/`
- `/usr/share/whisper/`
- `~/.whisper/`

Or specify custom path in `AudioParserConfig`:

```rust
let config = AudioParserConfig {
    model: "base".to_string(),
    model_path: Some("/path/to/your/model.bin".to_string()),
    ..Default::default()
};
let parser = AudioParser::new(config);
```

### 3. Install FFmpeg (for compressed audio/video)

For MP3, M4A, OGG, FLAC, WebM audio or video files:

```bash
# Arch Linux
sudo pacman -S ffmpeg

# Ubuntu/Debian
sudo apt install ffmpeg

# macOS
brew install ffmpeg
```

## Usage

### Audio Transcription

```rust
use cogkos_ingest::parser::{AudioParser, AudioParserConfig};

let config = AudioParserConfig {
    model: "base".to_string(),
    language: "auto".to_string(),
    ..Default::default()
};

let parser = AudioParser::new(config);
let result = parser.parse_file("audio.mp3").await?;

println!("Text: {}", result.text);
println!("Language: {}", result.language);
for segment in result.segments {
    println!("[{} - {}] {}", segment.start, segment.end, segment.text);
}
```

### Video Parsing

```rust
use cogkos_ingest::parser::{VideoParser, VideoParserConfig};

let config = VideoParserConfig {
    extract_audio: true,
    ..Default::default()
};

let parser = VideoParser::new(config);
let result = parser.parse_file("video.mp4").await?;

if let Some(transcription) = result.audio_transcription {
    println!("Audio: {}", transcription.text);
}
```

## Dependencies

### Rust Crates

- `whisper-rs` - Whisper binding (optional, requires `whisper` feature)
- `hound` - WAV file reading
- `shellexpand` - Path expansion

### System Libraries

- `libclang` - Required for whisper-rs-sys

## Limitations

1. **Compressed Audio**: MP3/M4A/OGG require FFmpeg for decoding
2. **Video Audio Extraction**: Requires FFmpeg for extracting audio from video containers
3. **Language Detection**: Auto-detection requires additional setup

## Troubleshooting

### "Could not find Whisper model"

Download a model and place it in the default location or specify `model_path` in config.

### "Failed to load Whisper model"

Ensure libclang is installed:
- Arch: `sudo pacman -S llvm`
- Ubuntu: `sudo apt install libclang-dev`
- macOS: `brew install llvm`

### "Audio transcription unavailable"

Make sure FFmpeg is installed for compressed audio formats.
