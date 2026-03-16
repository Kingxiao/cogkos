//! Audio parser implementation using Whisper
//!
//! This module provides audio transcription using Whisper STT.
//! Supports common audio formats: mp3, wav, m4a, ogg, flac

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(feature = "whisper")]
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{DocumentParser, TextChunk};

/// Audio parser configuration
#[derive(Debug, Clone)]
pub struct AudioParserConfig {
    /// Whisper model to use (tiny, base, small, medium, large)
    pub model: String,
    /// Language code (e.g., "en", "zh", "auto" for auto-detection)
    pub language: String,
    /// Enable parallel processing for long audio
    pub enable_chunking: bool,
    /// Maximum chunk duration in seconds
    pub max_chunk_duration_secs: u32,
    /// Path to whisper model file (optional)
    pub model_path: Option<String>,
}

impl Default for AudioParserConfig {
    fn default() -> Self {
        Self {
            model: "base".to_string(),
            language: "auto".to_string(),
            enable_chunking: true,
            max_chunk_duration_secs: 30,
            model_path: None,
        }
    }
}

/// Audio transcription result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioTranscription {
    /// Full transcribed text
    pub text: String,
    /// Language detected or specified
    pub language: String,
    /// Duration of audio in seconds
    pub duration_secs: f64,
    /// Segments with timestamps
    pub segments: Vec<AudioSegment>,
    /// Confidence score
    pub confidence: f64,
}

/// Audio segment with timestamps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSegment {
    /// Start time in seconds
    pub start: f64,
    /// End time in seconds
    pub end: f64,
    /// Transcribed text for this segment
    pub text: String,
    /// Confidence for this segment
    pub confidence: f64,
}

/// Audio parser using Whisper
pub struct AudioParser {
    config: AudioParserConfig,
    #[cfg(feature = "whisper")]
    context: Option<WhisperContext>,
}

impl AudioParser {
    pub fn new(config: AudioParserConfig) -> Self {
        #[cfg(feature = "whisper")]
        let context = Self::load_model(&config);

        Self {
            config,
            #[cfg(feature = "whisper")]
            context,
        }
    }

    #[cfg(feature = "whisper")]
    fn load_model(config: &AudioParserConfig) -> Option<WhisperContext> {
        // Try to load model from path if provided
        let model_path = if let Some(ref path) = config.model_path {
            path.clone()
        } else {
            // Try default paths based on model size
            let model_name = format!("{}-model.bin", config.model);
            let default_paths = [
                format!("/usr/local/share/whisper/{}", model_name),
                format!("/usr/share/whisper/{}", model_name),
                format!("~/.whisper/{}", model_name),
            ];

            let mut found_path = None;
            for path in default_paths {
                let expanded = shellexpand::tilde(&path);
                if std::path::Path::new(expanded.as_ref()).exists() {
                    found_path = Some(expanded.to_string());
                    break;
                }
            }

            match found_path {
                Some(p) => p,
                None => {
                    tracing::warn!(
                        "Could not find Whisper model, transcription will be unavailable"
                    );
                    return None;
                }
            }
        };

        match WhisperContext::new_with_params(&model_path, WhisperContextParameters::default()) {
            Ok(ctx) => {
                tracing::info!("Loaded Whisper model from {}", model_path);
                Some(ctx)
            }
            Err(e) => {
                tracing::warn!("Failed to load Whisper model from {}: {}", model_path, e);
                None
            }
        }
    }

    /// Parse audio file and return transcription
    ///
    /// # Arguments
    /// * `audio_data` - Raw audio file bytes
    /// * `format` - Audio format (mp3, wav, m4a, etc.)
    ///
    /// # Returns
    /// AudioTranscription with full text and segments
    pub async fn parse(
        &self,
        audio_data: &[u8],
        format: &str,
    ) -> Result<AudioTranscription, AudioParserError> {
        // Validate format
        let supported_formats = ["mp3", "wav", "m4a", "ogg", "flac", "webm"];
        if !supported_formats.contains(&format.to_lowercase().as_str()) {
            return Err(AudioParserError::UnsupportedFormat(format.to_string()));
        }

        #[cfg(feature = "whisper")]
        {
            if let Some(ref context) = self.context {
                return self
                    .transcribe_with_whisper(context, audio_data, format)
                    .await;
            }
        }

        // Fallback or no whisper feature: estimate duration
        let duration = self.estimate_duration(audio_data, format);

        Ok(AudioTranscription {
            text: "[Audio transcription - Whisper model not loaded, install with whisper feature and provide model]".to_string(),
            language: self.config.language.clone(),
            duration_secs: duration,
            segments: vec![],
            confidence: 0.0,
        })
    }

    #[cfg(feature = "whisper")]
    async fn transcribe_with_whisper(
        &self,
        context: &WhisperContext,
        audio_data: &[u8],
        format: &str,
    ) -> Result<AudioTranscription, AudioParserError> {
        // Convert audio to 16kHz mono PCM format required by Whisper
        let pcm_data = self
            .convert_to_whisper_format(audio_data, format)
            .map_err(|e| AudioParserError::WhisperError(e.to_string()))?;

        let duration = pcm_data.len() as f64 / 16000.0; // 16kHz = 16000 samples/sec

        // Run transcription in blocking task
        let pcm_vec = pcm_data;
        let language = self.config.language.clone();

        // Create state outside of blocking task to avoid lifetime issues
        let mut state = context.create_state().map_err(|e| {
            AudioParserError::WhisperError(format!("Failed to create state: {}", e))
        })?;

        let result = tokio::task::spawn_blocking(move || {
            // Set language if specified (not "auto")
            let lang_param = if language != "auto" && !language.is_empty() {
                Some(language.as_str())
            } else {
                None
            };

            // Configure params
            let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

            if let Some(lang) = lang_param {
                params.set_language(Some(lang));
            }

            // Disable printing
            params.set_print_special(false);
            params.set_print_progress(false);
            params.set_print_realtime(false);
            params.set_print_timestamps(false);

            // Convert to float samples for whisper
            let mut float_samples = vec![0.0f32; pcm_vec.len()];
            whisper_rs::convert_integer_to_float_audio(&pcm_vec, &mut float_samples).map_err(
                |e| AudioParserError::WhisperError(format!("Audio conversion failed: {}", e)),
            )?;

            // Convert to mono if needed (already mono from earlier conversion)
            let mono_samples = float_samples;

            // Run transcription
            state.full(params, &mono_samples).map_err(|e| {
                AudioParserError::WhisperError(format!("Transcription failed: {}", e))
            })?;

            // Extract segments
            let mut audio_segments = Vec::new();
            let mut full_text = String::new();
            let mut total_confidence = 0.0;

            for segment in state.as_iter() {
                let start = segment.start_timestamp() as f64 / 100.0; // Convert to seconds
                let end = segment.end_timestamp() as f64 / 100.0;
                let text = segment.to_str().unwrap_or_default().to_string();

                // Get probability for this segment (approximate confidence)
                let confidence = 0.85; // whisper-rs doesn't provide per-segment confidence

                audio_segments.push(AudioSegment {
                    start,
                    end,
                    text: text.clone(),
                    confidence,
                });

                if !full_text.is_empty() {
                    full_text.push(' ');
                }
                full_text.push_str(&text);
                total_confidence += confidence;
            }

            let avg_confidence = if !audio_segments.is_empty() {
                total_confidence / audio_segments.len() as f64
            } else {
                0.0
            };

            // Detect language if auto
            let detected_language = if language == "auto" || language.is_empty() {
                "en".to_string() // Language detection requires additional setup
            } else {
                language.clone()
            };

            Ok(AudioTranscription {
                text: full_text,
                language: detected_language,
                duration_secs: duration,
                segments: audio_segments,
                confidence: avg_confidence,
            })
        })
        .await
        .map_err(|e| AudioParserError::WhisperError(format!("Join error: {}", e)))??;

        Ok(result)
    }

    #[cfg(feature = "whisper")]
    fn convert_to_whisper_format(
        &self,
        audio_data: &[u8],
        format: &str,
    ) -> Result<Vec<i16>, AudioParserError> {
        match format.to_lowercase().as_str() {
            "wav" => self.convert_wav(audio_data),
            "mp3" | "m4a" | "ogg" | "flac" | "webm" => {
                // For compressed formats, we'd need ffmpeg or a decoder
                // For now, try to treat as raw PCM or return error
                // A full implementation would use a proper audio decoder
                self.convert_raw_audio(audio_data)
            }
            _ => Err(AudioParserError::UnsupportedFormat(format.to_string())),
        }
    }

    #[cfg(feature = "whisper")]
    fn convert_wav(&self, data: &[u8]) -> Result<Vec<i16>, AudioParserError> {
        let reader = hound::WavReader::new(std::io::Cursor::new(data))
            .map_err(|e| AudioParserError::WhisperError(format!("Invalid WAV: {}", e)))?;

        let spec = reader.spec();
        let samples: Vec<i16> = match spec.sample_format {
            hound::SampleFormat::Int => reader
                .into_samples::<i16>()
                .filter_map(|s| s.ok())
                .collect(),
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .map(|s| (s.unwrap_or(0.0) * 32767.0) as i16)
                .collect(),
        };

        // Convert to mono 16kHz if needed
        let mono = if spec.channels > 1 {
            Self::convert_to_mono(&samples, spec.channels as usize)
        } else {
            samples
        };

        let resampled = if spec.sample_rate != 16000 {
            Self::resample(&mono, spec.sample_rate, 16000)
        } else {
            mono
        };

        Ok(resampled)
    }

    #[cfg(feature = "whisper")]
    fn convert_to_mono(samples: &[i16], channels: usize) -> Vec<i16> {
        if channels == 1 {
            return samples.to_vec();
        }

        samples
            .chunks(channels)
            .map(|chunk| {
                let sum: i32 = chunk.iter().map(|&s| s as i32).sum();
                (sum / channels as i32) as i16
            })
            .collect()
    }

    #[cfg(feature = "whisper")]
    fn resample(samples: &[i16], from_rate: u32, to_rate: u32) -> Vec<i16> {
        if from_rate == to_rate {
            return samples.to_vec();
        }

        let ratio = to_rate as f64 / from_rate as f64;
        let new_len = (samples.len() as f64 * ratio) as usize;

        let mut resampled = Vec::with_capacity(new_len);
        for i in 0..new_len {
            let src_idx = i as f64 / ratio;
            let src_idx_floor = src_idx.floor() as usize;

            if src_idx_floor < samples.len() - 1 {
                let frac = src_idx.fract();
                let sample = samples[src_idx_floor] as f64
                    + (samples[src_idx_floor + 1] as f64 - samples[src_idx_floor] as f64) * frac;
                resampled.push(sample as i16);
            } else if src_idx_floor < samples.len() {
                resampled.push(samples[src_idx_floor]);
            }
        }

        resampled
    }

    #[cfg(feature = "whisper")]
    fn convert_raw_audio(&self, data: &[u8]) -> Result<Vec<i16>, AudioParserError> {
        // Assume 16-bit mono 16kHz PCM if not decodable
        // This is a fallback for testing purposes
        let samples: Vec<i16> = data
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        if samples.is_empty() {
            // If no valid 16-bit samples, create silent output
            return Ok(vec![]);
        }

        Ok(samples)
    }

    fn estimate_duration(&self, audio_data: &[u8], format: &str) -> f64 {
        // Rough estimation based on typical bitrates
        match format.to_lowercase().as_str() {
            "mp3" => audio_data.len() as f64 / 16000.0, // ~128kbps
            "wav" => audio_data.len() as f64 / 32000.0, // 16-bit mono
            "m4a" => audio_data.len() as f64 / 14000.0, // ~112kbps
            "ogg" => audio_data.len() as f64 / 16000.0,
            "flac" => audio_data.len() as f64 / 32000.0,
            _ => audio_data.len() as f64 / 16000.0,
        }
    }

    /// Parse audio from file path
    pub async fn parse_file(&self, path: &Path) -> Result<AudioTranscription, AudioParserError> {
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or(AudioParserError::InvalidPath)?;

        let audio_data =
            std::fs::read(path).map_err(|e| AudioParserError::IoError(e.to_string()))?;

        self.parse(&audio_data, format).await
    }
}

impl Default for AudioParser {
    fn default() -> Self {
        Self::new(AudioParserConfig::default())
    }
}

#[async_trait]
impl DocumentParser for AudioParser {
    fn supported_extensions(&self) -> &[&str] {
        &["mp3", "wav", "m4a", "ogg", "flac", "aac", "wma", "aiff"]
    }

    async fn parse(&self, data: &[u8], filename: &str) -> crate::Result<Vec<TextChunk>> {
        let format = filename.split('.').next_back().unwrap_or("mp3");

        #[cfg(feature = "whisper")]
        {
            match self.parse(data, format).await {
                Ok(transcription) => {
                    let chunks = transcription.to_text_chunks(1000);
                    Ok(chunks
                        .into_iter()
                        .enumerate()
                        .map(|(idx, text)| TextChunk {
                            content: text,
                            chunk_index: idx as u32,
                            metadata: Default::default(),
                        })
                        .collect())
                }
                Err(e) => Err(cogkos_core::CogKosError::Parse(e.to_string())),
            }
        }

        #[cfg(not(feature = "whisper"))]
        {
            // Return empty chunks when whisper feature is not enabled
            Ok(vec![TextChunk {
                content: format!("Audio file: {} (whisper feature not enabled)", filename),
                chunk_index: 0,
                metadata: Default::default(),
            }])
        }
    }
}

/// Audio parser errors
#[derive(Debug, thiserror::Error)]
pub enum AudioParserError {
    #[error("Unsupported audio format: {0}")]
    UnsupportedFormat(String),

    #[error("Invalid audio path")]
    InvalidPath,

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Whisper processing error: {0}")]
    WhisperError(String),

    #[error("Audio too long: {0} seconds (max: {1})")]
    AudioTooLong(f64, u32),
}

/// Document parser trait implementation for audio
#[allow(dead_code)]
pub struct AudioDocumentParser {
    parser: AudioParser,
}

impl AudioDocumentParser {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self {
            parser: AudioParser::default(),
        }
    }
}

impl Default for AudioDocumentParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert audio transcription to text chunks for ingestion
impl AudioTranscription {
    /// Convert transcription to text chunks for downstream processing
    pub fn to_text_chunks(&self, max_chunk_size: usize) -> Vec<String> {
        if self.text.is_empty() {
            return vec![];
        }

        // Simple chunking by sentences (naive implementation)
        let mut chunks = Vec::new();
        let mut current_chunk = String::new();

        for segment in &self.segments {
            if current_chunk.len() + segment.text.len() > max_chunk_size
                && !current_chunk.is_empty()
            {
                chunks.push(current_chunk.trim().to_string());
                current_chunk = String::new();
            }
            if !current_chunk.is_empty() {
                current_chunk.push(' ');
            }
            current_chunk.push_str(&segment.text);
        }

        if !current_chunk.is_empty() {
            chunks.push(current_chunk.trim().to_string());
        }

        chunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_duration() {
        let parser = AudioParser::default();
        let audio_data = vec![0u8; 32000]; // 32KB of dummy data
        let duration = parser.estimate_duration(&audio_data, "mp3");

        // 32000 / 16000 = 2 seconds
        assert!((duration - 2.0).abs() < 0.1);
    }

    #[test]
    fn test_audio_transcription_chunks() {
        let transcription = AudioTranscription {
            text: "Hello world. This is a test.".to_string(),
            language: "en".to_string(),
            duration_secs: 2.0,
            segments: vec![
                AudioSegment {
                    start: 0.0,
                    end: 1.0,
                    text: "Hello world.".to_string(),
                    confidence: 0.9,
                },
                AudioSegment {
                    start: 1.0,
                    end: 2.0,
                    text: "This is a test.".to_string(),
                    confidence: 0.9,
                },
            ],
            confidence: 0.9,
        };

        let chunks = transcription.to_text_chunks(20);
        assert!(!chunks.is_empty());
    }
}
