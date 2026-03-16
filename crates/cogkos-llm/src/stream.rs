use bytes::Bytes;
use futures::{Stream, StreamExt};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::{debug, trace};

pub struct StreamProcessor;

impl StreamProcessor {
    pub fn process_stream<S>(stream: S) -> ProcessedStream<S>
    where
        S: Stream<Item = crate::error::Result<String>>,
    {
        ProcessedStream {
            inner: stream,
            buffer: String::new(),
        }
    }

    pub fn combine_chunks(chunks: Vec<String>) -> String {
        chunks.join("")
    }

    pub fn estimate_tokens(text: &str) -> usize {
        // Rough estimation: 1 token ≈ 4 characters for English/Chinese
        text.chars().count() / 4
    }
}

#[pin_project]
pub struct ProcessedStream<S> {
    #[pin]
    inner: S,
    buffer: String,
}

impl<S> Stream for ProcessedStream<S>
where
    S: Stream<Item = crate::error::Result<String>>,
{
    type Item = crate::error::Result<String>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();

        match this.inner.poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.buffer.push_str(&chunk);
                trace!(
                    "Received chunk: {} chars, buffer now: {} chars",
                    chunk.len(),
                    this.buffer.len()
                );
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                debug!("Stream error: {}", e);
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                debug!("Stream complete, total buffer: {} chars", this.buffer.len());
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct StreamingResponse {
    pub content: String,
    pub chunks: Vec<String>,
    pub token_count: usize,
}

impl StreamingResponse {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            chunks: Vec::new(),
            token_count: 0,
        }
    }

    pub fn add_chunk(&mut self, chunk: String) {
        self.content.push_str(&chunk);
        self.chunks.push(chunk);
    }

    pub fn finalize(mut self) -> Self {
        self.token_count = StreamProcessor::estimate_tokens(&self.content);
        self
    }
}

impl Default for StreamingResponse {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn collect_stream<S>(mut stream: S) -> crate::error::Result<StreamingResponse>
where
    S: Stream<Item = crate::error::Result<String>> + Unpin,
{
    let mut response = StreamingResponse::new();

    while let Some(result) = stream.next().await {
        match result {
            Ok(chunk) => response.add_chunk(chunk),
            Err(e) => return Err(e),
        }
    }

    Ok(response.finalize())
}

pub fn create_sse_stream<S>(stream: S) -> impl Stream<Item = crate::error::Result<Bytes>>
where
    S: Stream<Item = crate::error::Result<String>>,
{
    stream.map(|result| {
        result.map(|chunk| {
            let sse_data = format!("data: {}\n\n", chunk);
            Bytes::from(sse_data)
        })
    })
}

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub max_tokens: Option<usize>,
    pub chunk_buffer_size: usize,
    pub enable_buffering: bool,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            max_tokens: None,
            chunk_buffer_size: 100,
            enable_buffering: true,
        }
    }
}

pub struct BufferedStream<S> {
    _inner: S,
    _config: StreamConfig,
    _buffer: Vec<String>,
    _total_tokens: usize,
}

impl<S> BufferedStream<S> {
    pub fn new(inner: S, config: StreamConfig) -> Self {
        let buffer_size = config.chunk_buffer_size;
        Self {
            _inner: inner,
            _config: config,
            _buffer: Vec::with_capacity(buffer_size),
            _total_tokens: 0,
        }
    }

    #[allow(dead_code)]
    fn should_flush(&self) -> bool {
        self._buffer.len() >= self._config.chunk_buffer_size
    }

    #[allow(dead_code)]
    fn flush_buffer(&mut self) -> Option<String> {
        if self._buffer.is_empty() {
            return None;
        }

        let content = self._buffer.join("");
        self._buffer.clear();
        Some(content)
    }
}
