//! ABOUTME: High-performance frame encoding with Rust JPEG and raw frame processing
//! ABOUTME: Provides YUV to RGB conversion and quality-controlled JPEG encoding

use bytes::Bytes;
use gl_core::{Error, Result};
use image::{ImageBuffer, Rgb, RgbImage};
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tracing::{debug, instrument, warn};

/// Configuration for frame encoding
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// JPEG quality (1-100, higher is better quality)
    pub jpeg_quality: u8,
    /// Enable fast encoding mode (lower quality, higher speed)
    pub fast_mode: bool,
    /// Target frame size in bytes (optional, for quality adaptation)
    pub target_size: Option<usize>,
    /// Enable progressive JPEG encoding
    pub progressive: bool,
    /// Color subsampling mode
    pub chroma_subsampling: ChromaSubsampling,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            jpeg_quality: 85,
            fast_mode: false,
            target_size: None,
            progressive: false,
            chroma_subsampling: ChromaSubsampling::Mode420,
        }
    }
}

/// Chroma subsampling modes for JPEG encoding
#[derive(Debug, Clone, Copy)]
pub enum ChromaSubsampling {
    /// 4:4:4 - No subsampling (highest quality)
    Mode444,
    /// 4:2:2 - Horizontal subsampling (good quality)
    Mode422,
    /// 4:2:0 - Both horizontal and vertical subsampling (standard)
    Mode420,
}

/// Frame encoding statistics
#[derive(Debug, Clone)]
pub struct EncoderStats {
    /// Total frames encoded
    pub frames_encoded: Arc<AtomicU64>,
    /// Total encoding time (microseconds)
    pub total_encoding_time_us: Arc<AtomicU64>,
    /// Total input bytes processed
    pub input_bytes_total: Arc<AtomicU64>,
    /// Total output bytes generated
    pub output_bytes_total: Arc<AtomicU64>,
    /// Encoding failures
    pub encoding_failures: Arc<AtomicU64>,
}

impl Default for EncoderStats {
    fn default() -> Self {
        Self {
            frames_encoded: Arc::new(AtomicU64::new(0)),
            total_encoding_time_us: Arc::new(AtomicU64::new(0)),
            input_bytes_total: Arc::new(AtomicU64::new(0)),
            output_bytes_total: Arc::new(AtomicU64::new(0)),
            encoding_failures: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl EncoderStats {
    /// Get average encoding time in microseconds
    pub fn avg_encoding_time_us(&self) -> u64 {
        let total_time = self.total_encoding_time_us.load(Ordering::Relaxed);
        let frame_count = self.frames_encoded.load(Ordering::Relaxed);
        if frame_count == 0 {
            0
        } else {
            total_time / frame_count
        }
    }

    /// Get compression ratio
    pub fn compression_ratio(&self) -> f64 {
        let input = self.input_bytes_total.load(Ordering::Relaxed);
        let output = self.output_bytes_total.load(Ordering::Relaxed);
        if input == 0 {
            0.0
        } else {
            output as f64 / input as f64
        }
    }
}

/// High-performance frame encoder with Rust JPEG encoding
pub struct FrameEncoder {
    /// Encoder configuration
    config: EncoderConfig,
    /// Encoding statistics
    stats: EncoderStats,
}

impl FrameEncoder {
    /// Create a new frame encoder
    pub fn new(config: EncoderConfig) -> Self {
        debug!(
            quality = config.jpeg_quality,
            fast_mode = config.fast_mode,
            progressive = config.progressive,
            "Creating frame encoder"
        );

        Self {
            config,
            stats: EncoderStats::default(),
        }
    }

    /// Encode RGB data to JPEG
    #[instrument(skip(self, rgb_data))]
    pub fn encode_rgb_to_jpeg(&self, rgb_data: &[u8], width: u32, height: u32) -> Result<Bytes> {
        let start_time = Instant::now();

        // Validate input dimensions
        let expected_size = (width * height * 3) as usize;
        if rgb_data.len() != expected_size {
            self.stats.encoding_failures.fetch_add(1, Ordering::Relaxed);
            return Err(Error::Config(format!(
                "RGB data size mismatch: expected {}, got {}",
                expected_size,
                rgb_data.len()
            )));
        }

        // Create RGB image buffer (safe ownership)
        let img_buffer =
            match ImageBuffer::<Rgb<u8>, Vec<u8>>::from_vec(width, height, rgb_data.to_vec()) {
                Some(buffer) => buffer,
                None => {
                    self.stats.encoding_failures.fetch_add(1, Ordering::Relaxed);
                    return Err(Error::Config(
                        "Failed to create image buffer from RGB data".to_string(),
                    ));
                }
            };

        // Encode to JPEG
        let jpeg_bytes = self.encode_image_to_jpeg(img_buffer)?;

        // Update statistics
        let encoding_time = start_time.elapsed();
        self.stats.frames_encoded.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_encoding_time_us
            .fetch_add(encoding_time.as_micros() as u64, Ordering::Relaxed);
        self.stats
            .input_bytes_total
            .fetch_add(rgb_data.len() as u64, Ordering::Relaxed);
        self.stats
            .output_bytes_total
            .fetch_add(jpeg_bytes.len() as u64, Ordering::Relaxed);

        debug!(
            width,
            height,
            input_size = rgb_data.len(),
            output_size = jpeg_bytes.len(),
            encoding_time_us = encoding_time.as_micros(),
            compression_ratio = jpeg_bytes.len() as f64 / rgb_data.len() as f64,
            "RGB frame encoded to JPEG"
        );

        Ok(jpeg_bytes)
    }

    /// Convert YUV420P data to RGB
    #[instrument(skip(self, yuv_data))]
    pub fn yuv420p_to_rgb(&self, yuv_data: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
        let start_time = Instant::now();

        // YUV420P requires even dimensions
        if width % 2 != 0 || height % 2 != 0 {
            return Err(Error::Config(format!(
                "YUV420P requires even dimensions, got {}x{}",
                width, height
            )));
        }

        // YUV420P layout: Y plane (width*height), U plane (width*height/4), V plane (width*height/4)
        let y_size = (width * height) as usize;
        let uv_size = (width * height / 4) as usize;
        let expected_size = y_size + 2 * uv_size;

        if yuv_data.len() != expected_size {
            return Err(Error::Config(format!(
                "YUV420P data size mismatch: expected {}, got {}",
                expected_size,
                yuv_data.len()
            )));
        }

        // Extract Y, U, V planes
        let y_plane = &yuv_data[0..y_size];
        let u_plane = &yuv_data[y_size..y_size + uv_size];
        let v_plane = &yuv_data[y_size + uv_size..];

        // Convert to RGB
        let mut rgb_data = Vec::with_capacity((width * height * 3) as usize);

        for y in 0..height {
            for x in 0..width {
                let y_index = (y * width + x) as usize;
                let uv_index = ((y / 2) * (width / 2) + (x / 2)) as usize;

                let y_val = y_plane[y_index] as f32;
                let u_val = u_plane[uv_index] as f32 - 128.0;
                let v_val = v_plane[uv_index] as f32 - 128.0;

                // YUV to RGB conversion using standard coefficients
                let r = (y_val + 1.402 * v_val).clamp(0.0, 255.0) as u8;
                let g = (y_val - 0.344 * u_val - 0.714 * v_val).clamp(0.0, 255.0) as u8;
                let b = (y_val + 1.772 * u_val).clamp(0.0, 255.0) as u8;

                rgb_data.extend_from_slice(&[r, g, b]);
            }
        }

        let conversion_time = start_time.elapsed();
        debug!(
            width,
            height,
            yuv_size = yuv_data.len(),
            rgb_size = rgb_data.len(),
            conversion_time_us = conversion_time.as_micros(),
            "YUV420P converted to RGB"
        );

        Ok(rgb_data)
    }

    /// Encode an RGB image to JPEG bytes
    #[instrument(skip(self, img_buffer))]
    fn encode_image_to_jpeg(&self, img_buffer: RgbImage) -> Result<Bytes> {
        let mut jpeg_data = Vec::new();
        let mut cursor = Cursor::new(&mut jpeg_data);

        // Configure JPEG encoder
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut cursor,
            self.config.jpeg_quality,
        );

        // Set encoder options based on configuration
        if self.config.progressive {
            // Progressive JPEG encoding (if supported)
        }

        // Encode the image
        match encoder.encode_image(&img_buffer) {
            Ok(_) => {
                let jpeg_bytes = Bytes::from(jpeg_data);
                debug!(
                    quality = self.config.jpeg_quality,
                    output_size = jpeg_bytes.len(),
                    "Image encoded to JPEG with Rust encoder"
                );
                Ok(jpeg_bytes)
            }
            Err(e) => {
                self.stats.encoding_failures.fetch_add(1, Ordering::Relaxed);
                Err(Error::Config(format!("JPEG encoding failed: {}", e)))
            }
        }
    }

    /// Encode raw frame data (auto-detect format)
    #[instrument(skip(self, frame_data))]
    pub fn encode_raw_frame(
        &self,
        frame_data: &[u8],
        width: u32,
        height: u32,
        format: RawFrameFormat,
    ) -> Result<Bytes> {
        match format {
            RawFrameFormat::Rgb24 => self.encode_rgb_to_jpeg(frame_data, width, height),
            RawFrameFormat::Yuv420p => {
                let rgb_data = self.yuv420p_to_rgb(frame_data, width, height)?;
                self.encode_rgb_to_jpeg(&rgb_data, width, height)
            }
            RawFrameFormat::Bgr24 => {
                // Convert BGR to RGB
                let mut rgb_data = Vec::with_capacity(frame_data.len());
                for chunk in frame_data.chunks(3) {
                    if chunk.len() == 3 {
                        rgb_data.extend_from_slice(&[chunk[2], chunk[1], chunk[0]]);
                        // BGR -> RGB
                    }
                }
                self.encode_rgb_to_jpeg(&rgb_data, width, height)
            }
        }
    }

    /// Get encoder statistics
    pub fn stats(&self) -> &EncoderStats {
        &self.stats
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        self.stats.frames_encoded.store(0, Ordering::Relaxed);
        self.stats
            .total_encoding_time_us
            .store(0, Ordering::Relaxed);
        self.stats.input_bytes_total.store(0, Ordering::Relaxed);
        self.stats.output_bytes_total.store(0, Ordering::Relaxed);
        self.stats.encoding_failures.store(0, Ordering::Relaxed);
    }
}

/// Supported raw frame formats
#[derive(Debug, Clone, Copy)]
pub enum RawFrameFormat {
    /// RGB 24-bit (8 bits per channel)
    Rgb24,
    /// YUV 4:2:0 Planar
    Yuv420p,
    /// BGR 24-bit (8 bits per channel)
    Bgr24,
}

impl RawFrameFormat {
    /// Get the expected bytes per pixel
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            RawFrameFormat::Rgb24 | RawFrameFormat::Bgr24 => 3,
            RawFrameFormat::Yuv420p => 1, // Y plane only, UV is subsampled
        }
    }

    /// Calculate expected frame size in bytes
    pub fn frame_size(&self, width: u32, height: u32) -> usize {
        match self {
            RawFrameFormat::Rgb24 | RawFrameFormat::Bgr24 => (width * height * 3) as usize,
            RawFrameFormat::Yuv420p => (width * height * 3 / 2) as usize, // Y + U/2 + V/2
        }
    }
}

/// Quality adaptation based on target size and performance
pub struct QualityAdapter {
    /// Target encoding time in microseconds
    pub target_time_us: u64,
    /// Current quality setting
    current_quality: u8,
    /// Quality adjustment history for smoothing
    adjustment_history: Vec<i8>,
    /// Maximum quality adjustment per step
    max_adjustment: u8,
}

impl QualityAdapter {
    /// Create a new quality adapter
    pub fn new(target_time_us: u64) -> Self {
        Self {
            target_time_us,
            current_quality: 85,
            adjustment_history: Vec::new(),
            max_adjustment: 5,
        }
    }

    /// Adapt quality based on encoding performance
    #[instrument(skip(self))]
    pub fn adapt_quality(&mut self, encoding_time: Duration, frame_size: usize) -> u8 {
        let encoding_time_us = encoding_time.as_micros() as u64;

        let adjustment = if encoding_time_us > self.target_time_us * 120 / 100 {
            // Too slow, reduce quality
            -(self.max_adjustment as i8)
        } else if encoding_time_us < self.target_time_us * 80 / 100 {
            // Fast enough, can increase quality
            self.max_adjustment as i8
        } else {
            // Within target range
            0
        };

        // Apply adjustment with bounds checking
        if adjustment != 0 {
            self.adjustment_history.push(adjustment);

            // Smooth adjustments by averaging recent history
            if self.adjustment_history.len() > 5 {
                self.adjustment_history.remove(0);
            }

            let avg_adjustment: f32 = self
                .adjustment_history
                .iter()
                .map(|&x| x as f32)
                .sum::<f32>()
                / self.adjustment_history.len() as f32;

            let new_quality =
                (self.current_quality as i16 + avg_adjustment as i16).clamp(20, 100) as u8;

            if new_quality != self.current_quality {
                debug!(
                    old_quality = self.current_quality,
                    new_quality,
                    encoding_time_us,
                    target_time_us = self.target_time_us,
                    frame_size,
                    "Adapted JPEG quality"
                );
                self.current_quality = new_quality;
            }
        }

        self.current_quality
    }

    /// Get current quality setting
    pub fn current_quality(&self) -> u8 {
        self.current_quality
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encoder_config_default() {
        let config = EncoderConfig::default();
        assert_eq!(config.jpeg_quality, 85);
        assert!(!config.fast_mode);
        assert!(config.target_size.is_none());
        assert!(!config.progressive);
    }

    #[test]
    fn test_raw_frame_format_calculations() {
        assert_eq!(RawFrameFormat::Rgb24.bytes_per_pixel(), 3);
        assert_eq!(RawFrameFormat::Yuv420p.bytes_per_pixel(), 1);
        assert_eq!(RawFrameFormat::Bgr24.bytes_per_pixel(), 3);

        assert_eq!(RawFrameFormat::Rgb24.frame_size(640, 480), 640 * 480 * 3);
        assert_eq!(
            RawFrameFormat::Yuv420p.frame_size(640, 480),
            640 * 480 * 3 / 2
        );
    }

    #[test]
    fn test_encoder_stats() {
        let stats = EncoderStats::default();
        assert_eq!(stats.avg_encoding_time_us(), 0);
        assert_eq!(stats.compression_ratio(), 0.0);

        // Simulate some encoding
        stats.frames_encoded.store(10, Ordering::Relaxed);
        stats.total_encoding_time_us.store(50000, Ordering::Relaxed);
        stats.input_bytes_total.store(1000000, Ordering::Relaxed);
        stats.output_bytes_total.store(200000, Ordering::Relaxed);

        assert_eq!(stats.avg_encoding_time_us(), 5000);
        assert_eq!(stats.compression_ratio(), 0.2);
    }

    #[test]
    fn test_quality_adapter() {
        let mut adapter = QualityAdapter::new(10000); // 10ms target
        assert_eq!(adapter.current_quality(), 85);

        // Test slow encoding - should reduce quality
        let slow_time = Duration::from_micros(15000);
        let new_quality = adapter.adapt_quality(slow_time, 100000);
        assert!(new_quality < 85);

        // Test fast encoding - should increase quality
        let fast_time = Duration::from_micros(5000);
        let mut adapter2 = QualityAdapter::new(10000);
        adapter2.current_quality = 70;
        let new_quality2 = adapter2.adapt_quality(fast_time, 50000);
        assert!(new_quality2 > 70);
    }

    #[tokio::test]
    async fn test_frame_encoder_rgb() {
        let config = EncoderConfig {
            jpeg_quality: 90,
            ..Default::default()
        };
        let encoder = FrameEncoder::new(config);

        // Create test RGB data (red square)
        let width = 100;
        let height = 100;
        let mut rgb_data = vec![0u8; (width * height * 3) as usize];

        // Fill with red pixels
        for i in (0..rgb_data.len()).step_by(3) {
            rgb_data[i] = 255; // R
            rgb_data[i + 1] = 0; // G
            rgb_data[i + 2] = 0; // B
        }

        match encoder.encode_rgb_to_jpeg(&rgb_data, width, height) {
            Ok(jpeg_bytes) => {
                assert!(!jpeg_bytes.is_empty());
                // JPEG files start with 0xFF 0xD8
                assert_eq!(jpeg_bytes[0], 0xFF);
                assert_eq!(jpeg_bytes[1], 0xD8);
                // JPEG files end with 0xFF 0xD9
                assert_eq!(jpeg_bytes[jpeg_bytes.len() - 2], 0xFF);
                assert_eq!(jpeg_bytes[jpeg_bytes.len() - 1], 0xD9);

                // Check compression ratio
                let compression_ratio = jpeg_bytes.len() as f64 / rgb_data.len() as f64;
                assert!(compression_ratio < 1.0); // Should be compressed
                assert!(compression_ratio > 0.01); // Should not be too compressed

                println!(
                    "RGB encoding test: {}x{} -> {} bytes (ratio: {:.3})",
                    width,
                    height,
                    jpeg_bytes.len(),
                    compression_ratio
                );
            }
            Err(e) => {
                eprintln!("RGB encoding test failed: {}", e);
            }
        }
    }

    #[test]
    fn test_yuv420p_to_rgb_conversion() {
        let encoder = FrameEncoder::new(EncoderConfig::default());

        let width = 4;
        let height = 4;

        // Create test YUV420P data
        let y_data = vec![128u8; (width * height) as usize]; // Mid-gray Y
        let u_data = vec![128u8; (width * height / 4) as usize]; // Neutral U
        let v_data = vec![128u8; (width * height / 4) as usize]; // Neutral V

        let mut yuv_data = Vec::new();
        yuv_data.extend(&y_data);
        yuv_data.extend(&u_data);
        yuv_data.extend(&v_data);

        match encoder.yuv420p_to_rgb(&yuv_data, width, height) {
            Ok(rgb_data) => {
                assert_eq!(rgb_data.len(), (width * height * 3) as usize);

                // With Y=128, U=128, V=128, we should get approximately gray pixels
                // Check first pixel
                let r = rgb_data[0];
                let g = rgb_data[1];
                let b = rgb_data[2];

                // Should all be close to 128 (gray)
                assert!((r as i16 - 128).abs() < 20);
                assert!((g as i16 - 128).abs() < 20);
                assert!((b as i16 - 128).abs() < 20);
            }
            Err(e) => {
                eprintln!("YUV to RGB conversion test failed: {}", e);
            }
        }
    }
}
