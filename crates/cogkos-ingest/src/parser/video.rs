//! Video parser implementation
//!
//! This module provides video parsing by extracting audio and transcribing.
//! Supports common video formats: mp4, mov, avi, mkv, webm

use crate::parser::AudioParser;
use crate::{DocumentParser, TextChunk};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Video parser configuration
#[derive(Debug, Clone)]
pub struct VideoParserConfig {
    /// Enable audio extraction
    pub extract_audio: bool,
    /// Enable keyframe extraction
    pub extract_keyframes: bool,
    /// Maximum keyframes to extract
    pub max_keyframes: usize,
    /// Audio parser config
    pub audio_config: crate::parser::AudioParserConfig,
}

impl Default for VideoParserConfig {
    fn default() -> Self {
        Self {
            extract_audio: true,
            extract_keyframes: false,
            max_keyframes: 10,
            audio_config: crate::parser::AudioParserConfig::default(),
        }
    }
}

/// Video parsing result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoParseResult {
    /// Duration of video in seconds
    pub duration_secs: f64,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Frame rate
    pub fps: f64,
    /// Audio transcription (if audio extraction enabled)
    pub audio_transcription: Option<crate::parser::AudioTranscription>,
    /// Extracted keyframes (if keyframe extraction enabled)
    pub keyframes: Vec<KeyFrame>,
}

/// Extracted keyframe
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyFrame {
    /// Timestamp in seconds
    pub timestamp: f64,
    /// Image data ( JPEG)
    pub image_data: Vec<u8>,
}

/// Video parser
pub struct VideoParser {
    config: VideoParserConfig,
    audio_parser: AudioParser,
}

impl VideoParser {
    pub fn new(config: VideoParserConfig) -> Self {
        Self {
            audio_parser: AudioParser::new(config.audio_config.clone()),
            config,
        }
    }

    /// Parse video file and extract audio for transcription
    ///
    /// # Arguments
    /// * `video_data` - Raw video file bytes
    /// * `format` - Video format (mp4, mov, avi, etc.)
    ///
    /// # Returns
    /// VideoParseResult with audio transcription
    pub async fn parse(
        &self,
        video_data: &[u8],
        format: &str,
    ) -> Result<VideoParseResult, VideoParserError> {
        // Validate format
        let supported_formats = ["mp4", "mov", "avi", "mkv", "webm"];
        if !supported_formats.contains(&format.to_lowercase().as_str()) {
            return Err(VideoParserError::UnsupportedFormat(format.to_string()));
        }

        // Extract video metadata (basic parsing for common formats)
        let (duration, width, height, fps) = self.parse_video_metadata(video_data, format);

        // Extract audio if enabled
        let audio_transcription = if self.config.extract_audio {
            match self.extract_audio_from_video(video_data, format).await {
                Ok(audio_data) => {
                    // Use AudioParser to transcribe
                    Some(
                        self.audio_parser
                            .parse(&audio_data, "wav")
                            .await
                            .unwrap_or_else(|e| {
                                tracing::warn!("Audio transcription failed: {}", e);
                                crate::parser::AudioTranscription {
                                    text: "[Audio extraction failed]".to_string(),
                                    language: "unknown".to_string(),
                                    duration_secs: duration,
                                    segments: vec![],
                                    confidence: 0.0,
                                }
                            }),
                    )
                }
                Err(e) => {
                    tracing::warn!("Failed to extract audio: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Extract keyframes if enabled (placeholder - would need ffmpeg/opencv)
        let keyframes: Vec<KeyFrame> = vec![];

        Ok(VideoParseResult {
            duration_secs: duration,
            width,
            height,
            fps,
            audio_transcription,
            keyframes,
        })
    }

    /// Parse basic video metadata from container
    fn parse_video_metadata(&self, video_data: &[u8], format: &str) -> (f64, u32, u32, f64) {
        // Basic MP4/MOV parsing to extract duration and dimensions
        // This is a simplified implementation
        match format.to_lowercase().as_str() {
            "mp4" | "mov" => self.parse_mp4_metadata(video_data),
            "avi" => (0.0, 0, 0, 0.0), // AVI parsing would require more complex implementation
            "mkv" | "webm" => (0.0, 0, 0, 0.0),
            _ => (0.0, 0, 0, 0.0),
        }
    }

    /// Parse MP4/MOV container metadata
    fn parse_mp4_metadata(&self, data: &[u8]) -> (f64, u32, u32, f64) {
        // Very basic MP4 parsing - look for common atoms
        // In production, use a proper MP4 parser

        // Estimate duration based on file size (rough approximation)
        // Assume ~1MB per second for HD video
        let estimated_duration = data.len() as f64 / 1_000_000.0;

        // Default dimensions (would be parsed from actual atoms)
        let (width, height) = (1920, 1080); // Default to HD
        let fps = 30.0;

        (estimated_duration, width, height, fps)
    }

    /// Extract audio from video data using ffmpeg subprocess
    async fn extract_audio_from_video(
        &self,
        video_data: &[u8],
        format: &str,
    ) -> Result<Vec<u8>, VideoParserError> {
        // Check ffmpeg availability
        let ffmpeg_check = tokio::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .await;
        if ffmpeg_check.is_err() {
            return Err(VideoParserError::FfmpegError(
                "ffmpeg not found in PATH".to_string(),
            ));
        }

        // Write video data to a temp file (ffmpeg needs seekable input for most containers)
        let tmp_dir = std::env::temp_dir();
        let input_id = uuid::Uuid::new_v4();
        let input_path = tmp_dir.join(format!("cogkos_video_{}.{}", input_id, format));
        let output_path = tmp_dir.join(format!("cogkos_audio_{}.wav", input_id));

        tokio::fs::write(&input_path, video_data)
            .await
            .map_err(|e| VideoParserError::IoError(format!("Failed to write temp video: {}", e)))?;

        // Run ffmpeg: extract audio → 16kHz mono WAV (Whisper-compatible)
        let result = tokio::process::Command::new("ffmpeg")
            .args([
                "-i",
                input_path.to_str().unwrap_or("input"),
                "-vn",           // no video
                "-acodec",
                "pcm_s16le",     // 16-bit PCM
                "-ar",
                "16000",         // 16kHz sample rate
                "-ac",
                "1",             // mono
                "-y",            // overwrite
                output_path.to_str().unwrap_or("output"),
            ])
            .output()
            .await
            .map_err(|e| VideoParserError::FfmpegError(format!("ffmpeg execution failed: {}", e)))?;

        // Clean up input immediately
        let _ = tokio::fs::remove_file(&input_path).await;

        if !result.status.success() {
            let _ = tokio::fs::remove_file(&output_path).await;
            let stderr = String::from_utf8_lossy(&result.stderr);
            return Err(VideoParserError::FfmpegError(format!(
                "ffmpeg exited with {}: {}",
                result.status,
                stderr.chars().take(500).collect::<String>()
            )));
        }

        // Read extracted WAV
        let wav_data = tokio::fs::read(&output_path)
            .await
            .map_err(|e| VideoParserError::IoError(format!("Failed to read extracted audio: {}", e)))?;

        let _ = tokio::fs::remove_file(&output_path).await;

        if wav_data.is_empty() {
            return Err(VideoParserError::FfmpegError(
                "ffmpeg produced empty audio output (video may have no audio track)".to_string(),
            ));
        }

        Ok(wav_data)
    }

    /// Parse video from file path
    pub async fn parse_file(&self, path: &Path) -> Result<VideoParseResult, VideoParserError> {
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or(VideoParserError::InvalidPath)?;

        let video_data =
            std::fs::read(path).map_err(|e| VideoParserError::IoError(e.to_string()))?;

        self.parse(&video_data, format).await
    }
}

impl Default for VideoParser {
    fn default() -> Self {
        Self::new(VideoParserConfig::default())
    }
}

#[async_trait]
impl DocumentParser for VideoParser {
    fn supported_extensions(&self) -> &[&str] {
        &["mp4", "mov", "avi", "mkv", "webm", "wmv", "flv", "m4v"]
    }

    async fn parse(&self, data: &[u8], filename: &str) -> crate::Result<Vec<TextChunk>> {
        let ext = std::path::Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4");

        let result = self.parse(data, ext).await.map_err(|e| {
            cogkos_core::CogKosError::Parse(format!("Video parsing failed for {}: {}", filename, e))
        })?;

        let mut chunks = Vec::new();

        if let Some(ref transcription) = result.audio_transcription
            && !transcription.text.is_empty() {
                let mut metadata = std::collections::HashMap::new();
                metadata.insert("source".to_string(), filename.to_string());
                metadata.insert("type".to_string(), "video_transcription".to_string());
                metadata.insert(
                    "duration_secs".to_string(),
                    format!("{:.1}", result.duration_secs),
                );
                metadata.insert("width".to_string(), result.width.to_string());
                metadata.insert("height".to_string(), result.height.to_string());

                chunks.push(TextChunk {
                    content: transcription.text.clone(),
                    chunk_index: 0,
                    metadata,
                });
            }

        if chunks.is_empty() {
            tracing::warn!("No text content extracted from video: {}", filename);
        }

        Ok(chunks)
    }
}

/// Video parser errors
#[derive(Debug, thiserror::Error)]
pub enum VideoParserError {
    #[error("Unsupported video format: {0}")]
    UnsupportedFormat(String),

    #[error("Invalid video path")]
    InvalidPath,

    #[error("IO error: {0}")]
    IoError(String),

    #[error("FFmpeg error: {0}")]
    FfmpegError(String),

    #[error("Video too long: {0} seconds (max: {1})")]
    VideoTooLong(f64, u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_parser_creation() {
        let parser = VideoParser::default();
        assert!(parser.config.extract_audio);
    }

    #[tokio::test]
    async fn test_unsupported_format() {
        let parser = VideoParser::default();
        let result = parser.parse(b"fake video data", "flv").await;
        assert!(result.is_err());
    }
}
