//! ABOUTME: Zero-copy frame buffer management for high-performance MJPEG streaming
//! ABOUTME: Provides shared buffer pools and reference-counted frame data to eliminate memory allocations

use bytes::Bytes;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

/// Metadata associated with a frame
#[derive(Debug, Clone)]
pub struct FrameMetadata {
    /// Unique frame identifier
    pub id: Uuid,
    /// Timestamp when frame was created
    pub timestamp: Instant,
    /// Frame sequence number
    pub sequence: u64,
    /// Frame size in bytes
    pub size: usize,
    /// JPEG quality used for encoding
    pub quality: u8,
    /// Source identifier (for debugging)
    pub source_id: String,
}

impl FrameMetadata {
    /// Create new frame metadata
    pub fn new(sequence: u64, size: usize, quality: u8, source_id: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Instant::now(),
            sequence,
            size,
            quality,
            source_id,
        }
    }

    /// Get the age of this frame
    pub fn age(&self) -> Duration {
        self.timestamp.elapsed()
    }
}

/// A zero-copy frame buffer with reference counting
#[derive(Debug, Clone)]
pub struct FrameBuffer {
    /// Frame data (shared via Arc)
    pub data: Arc<[u8]>,
    /// Frame metadata
    pub metadata: FrameMetadata,
}

impl FrameBuffer {
    /// Create a new frame buffer from bytes
    pub fn new(data: Bytes, metadata: FrameMetadata) -> Self {
        // Use Bytes directly since it's already optimized for zero-copy sharing
        // Convert only the slice to Arc for consistent interface
        let arc_data: Arc<[u8]> = Arc::from(&data[..]);

        Self {
            data: arc_data,
            metadata,
        }
    }

    /// Create from raw Vec<u8>
    pub fn from_vec(data: Vec<u8>, metadata: FrameMetadata) -> Self {
        let arc_data: Arc<[u8]> = data.into();

        Self {
            data: arc_data,
            metadata,
        }
    }

    /// Get frame data as Bytes (zero-copy)
    pub fn as_bytes(&self) -> Bytes {
        Bytes::copy_from_slice(&self.data)
    }

    /// Get frame size
    pub fn size(&self) -> usize {
        self.data.len()
    }

    /// Check if frame is valid JPEG
    pub fn is_valid_jpeg(&self) -> bool {
        self.data.len() >= 2 && self.data[0] == 0xFF && self.data[1] == 0xD8
    }

    /// Get reference count (for debugging)
    pub fn ref_count(&self) -> usize {
        Arc::strong_count(&self.data)
    }
}

/// Configuration for buffer pool
#[derive(Debug, Clone)]
pub struct BufferPoolConfig {
    /// Maximum number of buffers in the pool
    pub max_buffers: usize,
    /// Buffer size for small frames (640x480 JPEG ~50KB)
    pub small_buffer_size: usize,
    /// Buffer size for medium frames (1280x720 JPEG ~150KB)
    pub medium_buffer_size: usize,
    /// Buffer size for large frames (1920x1080 JPEG ~300KB)
    pub large_buffer_size: usize,
    /// Maximum age before buffer recycling
    pub max_buffer_age: Duration,
}

impl Default for BufferPoolConfig {
    fn default() -> Self {
        Self {
            max_buffers: 20,
            small_buffer_size: 64 * 1024,   // 64KB
            medium_buffer_size: 192 * 1024, // 192KB
            large_buffer_size: 384 * 1024,  // 384KB
            max_buffer_age: Duration::from_secs(30),
        }
    }
}

/// Buffer pool statistics
#[derive(Debug, Clone)]
pub struct BufferPoolStats {
    /// Total allocations made
    pub total_allocations: Arc<AtomicU64>,
    /// Total recycled buffers used
    pub total_recycled: Arc<AtomicU64>,
    /// Current pool size
    pub current_pool_size: Arc<AtomicUsize>,
    /// Peak pool size
    pub peak_pool_size: Arc<AtomicUsize>,
    /// Total memory allocated (bytes)
    pub total_memory_bytes: Arc<AtomicU64>,
}

impl Default for BufferPoolStats {
    fn default() -> Self {
        Self {
            total_allocations: Arc::new(AtomicU64::new(0)),
            total_recycled: Arc::new(AtomicU64::new(0)),
            current_pool_size: Arc::new(AtomicUsize::new(0)),
            peak_pool_size: Arc::new(AtomicUsize::new(0)),
            total_memory_bytes: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Trait for buffer allocators
pub trait BufferAllocator: Send + Sync + std::fmt::Debug {
    /// Allocate a buffer of the specified size
    fn allocate(&self, size: usize) -> Vec<u8>;

    /// Get allocator name for metrics
    fn name(&self) -> &'static str;
}

/// Standard heap allocator
#[derive(Debug)]
pub struct HeapAllocator;

impl BufferAllocator for HeapAllocator {
    fn allocate(&self, size: usize) -> Vec<u8> {
        vec![0u8; size]
    }

    fn name(&self) -> &'static str {
        "heap"
    }
}

/// Recycled buffer entry
#[derive(Debug)]
struct RecycledBuffer {
    /// Buffer data
    data: Vec<u8>,
    /// When this buffer was recycled
    recycled_at: Instant,
    /// Original capacity
    capacity: usize,
}

/// High-performance buffer pool for zero-copy frame operations
pub struct BufferPool {
    /// Pool configuration
    config: BufferPoolConfig,
    /// Available recycled buffers by size category
    small_buffers: Arc<Mutex<VecDeque<RecycledBuffer>>>,
    medium_buffers: Arc<Mutex<VecDeque<RecycledBuffer>>>,
    large_buffers: Arc<Mutex<VecDeque<RecycledBuffer>>>,
    /// Buffer allocator
    allocator: Box<dyn BufferAllocator>,
    /// Pool statistics
    stats: BufferPoolStats,
}

impl std::fmt::Debug for BufferPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferPool")
            .field("config", &self.config)
            .field("stats", &self.stats)
            .field("allocator", &self.allocator.name())
            .finish()
    }
}

impl BufferPool {
    /// Create a new buffer pool
    pub fn new(config: BufferPoolConfig) -> Self {
        info!(
            max_buffers = config.max_buffers,
            small_size = config.small_buffer_size,
            medium_size = config.medium_buffer_size,
            large_size = config.large_buffer_size,
            "Creating buffer pool"
        );

        Self {
            config,
            small_buffers: Arc::new(Mutex::new(VecDeque::new())),
            medium_buffers: Arc::new(Mutex::new(VecDeque::new())),
            large_buffers: Arc::new(Mutex::new(VecDeque::new())),
            allocator: Box::new(HeapAllocator),
            stats: BufferPoolStats::default(),
        }
    }

    /// Create with custom allocator
    pub fn with_allocator(config: BufferPoolConfig, allocator: Box<dyn BufferAllocator>) -> Self {
        let mut pool = Self::new(config);
        pool.allocator = allocator;
        pool
    }

    /// Get an appropriately sized buffer for the given size
    #[instrument(skip(self))]
    pub async fn get_buffer(&self, required_size: usize) -> Vec<u8> {
        // Determine buffer category
        let (target_size, buffers) = if required_size <= self.config.small_buffer_size {
            (self.config.small_buffer_size, &self.small_buffers)
        } else if required_size <= self.config.medium_buffer_size {
            (self.config.medium_buffer_size, &self.medium_buffers)
        } else {
            (
                self.config.large_buffer_size.max(required_size),
                &self.large_buffers,
            )
        };

        // Try to get a recycled buffer
        {
            let mut pool = buffers.lock().await;
            if let Some(recycled) = pool.pop_front() {
                // Check if buffer is still valid (not too old)
                if recycled.recycled_at.elapsed() < self.config.max_buffer_age
                    && recycled.capacity >= required_size
                {
                    self.stats.total_recycled.fetch_add(1, Ordering::Relaxed);
                    let current_size = pool.len();
                    self.stats
                        .current_pool_size
                        .store(current_size, Ordering::Relaxed);

                    debug!(
                        required_size,
                        buffer_capacity = recycled.capacity,
                        pool_size = current_size,
                        "Reused buffer from pool"
                    );

                    let mut buffer = recycled.data;
                    buffer.clear();
                    buffer.resize(required_size, 0);
                    return buffer;
                }
            }
        }

        // No suitable buffer available, allocate new one
        self.stats.total_allocations.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_memory_bytes
            .fetch_add(target_size as u64, Ordering::Relaxed);

        debug!(
            required_size,
            target_size,
            allocator = self.allocator.name(),
            "Allocating new buffer"
        );

        self.allocator.allocate(target_size)
    }

    /// Return a buffer to the pool for recycling
    #[instrument(skip(self, buffer))]
    pub async fn return_buffer(&self, buffer: Vec<u8>) {
        let capacity = buffer.capacity();

        // Determine which pool this buffer belongs to
        let buffers = if capacity <= self.config.small_buffer_size * 2 {
            &self.small_buffers
        } else if capacity <= self.config.medium_buffer_size * 2 {
            &self.medium_buffers
        } else {
            &self.large_buffers
        };

        let mut pool = buffers.lock().await;

        // Check if we have space in the pool
        if pool.len() < self.config.max_buffers {
            let recycled = RecycledBuffer {
                data: buffer,
                recycled_at: Instant::now(),
                capacity,
            };

            pool.push_back(recycled);
            let new_size = pool.len();
            self.stats
                .current_pool_size
                .store(new_size, Ordering::Relaxed);

            // Update peak size
            let current_peak = self.stats.peak_pool_size.load(Ordering::Relaxed);
            if new_size > current_peak {
                self.stats.peak_pool_size.store(new_size, Ordering::Relaxed);
            }

            debug!(capacity, pool_size = new_size, "Buffer returned to pool");
        } else {
            debug!(
                capacity,
                max_buffers = self.config.max_buffers,
                "Pool full, dropping buffer"
            );
        }
    }

    /// Create a frame buffer with zero-copy data sharing
    #[instrument(skip(self, data))]
    pub async fn create_frame(&self, data: Bytes, metadata: FrameMetadata) -> FrameBuffer {
        debug!(
            frame_id = %metadata.id,
            size = data.len(),
            sequence = metadata.sequence,
            "Creating zero-copy frame buffer"
        );

        FrameBuffer::new(data, metadata)
    }

    /// Get pool statistics
    pub fn stats(&self) -> &BufferPoolStats {
        &self.stats
    }

    /// Clean up old buffers from the pool
    #[instrument(skip(self))]
    pub async fn cleanup_old_buffers(&self) {
        let cutoff_time = Instant::now() - self.config.max_buffer_age;
        let pools = [
            &self.small_buffers,
            &self.medium_buffers,
            &self.large_buffers,
        ];

        for buffers in pools {
            let mut pool = buffers.lock().await;
            let initial_size = pool.len();

            // Remove old buffers
            pool.retain(|buf| buf.recycled_at > cutoff_time);

            let removed = initial_size - pool.len();
            if removed > 0 {
                debug!(
                    removed_buffers = removed,
                    remaining = pool.len(),
                    "Cleaned up old buffers from pool"
                );
            }
        }

        // Update current pool size stats
        let total_size = self.small_buffers.lock().await.len()
            + self.medium_buffers.lock().await.len()
            + self.large_buffers.lock().await.len();
        self.stats
            .current_pool_size
            .store(total_size, Ordering::Relaxed);
    }

    /// Get detailed pool information for monitoring
    pub async fn get_pool_info(&self) -> BufferPoolInfo {
        let small_count = self.small_buffers.lock().await.len();
        let medium_count = self.medium_buffers.lock().await.len();
        let large_count = self.large_buffers.lock().await.len();

        BufferPoolInfo {
            small_buffers: small_count,
            medium_buffers: medium_count,
            large_buffers: large_count,
            total_buffers: small_count + medium_count + large_count,
            total_allocations: self.stats.total_allocations.load(Ordering::Relaxed),
            total_recycled: self.stats.total_recycled.load(Ordering::Relaxed),
            peak_pool_size: self.stats.peak_pool_size.load(Ordering::Relaxed),
            total_memory_bytes: self.stats.total_memory_bytes.load(Ordering::Relaxed),
            recycling_efficiency: self.recycling_efficiency(),
        }
    }

    /// Calculate recycling efficiency (percentage of recycled vs allocated)
    fn recycling_efficiency(&self) -> f64 {
        let total_allocations = self.stats.total_allocations.load(Ordering::Relaxed);
        let total_recycled = self.stats.total_recycled.load(Ordering::Relaxed);

        if total_allocations == 0 {
            0.0
        } else {
            (total_recycled as f64 / total_allocations as f64) * 100.0
        }
    }
}

/// Detailed buffer pool information for monitoring
#[derive(Debug, Clone)]
pub struct BufferPoolInfo {
    pub small_buffers: usize,
    pub medium_buffers: usize,
    pub large_buffers: usize,
    pub total_buffers: usize,
    pub total_allocations: u64,
    pub total_recycled: u64,
    pub peak_pool_size: usize,
    pub total_memory_bytes: u64,
    pub recycling_efficiency: f64,
}

/// Manages multiple buffer pools for different frame types
pub struct BufferPoolManager {
    /// Buffer pools by source type
    pools: Arc<Mutex<std::collections::HashMap<String, Arc<BufferPool>>>>,
    /// Default pool configuration
    default_config: BufferPoolConfig,
}

impl BufferPoolManager {
    /// Create a new buffer pool manager
    pub fn new(default_config: BufferPoolConfig) -> Self {
        info!("Creating buffer pool manager");

        Self {
            pools: Arc::new(Mutex::new(std::collections::HashMap::new())),
            default_config,
        }
    }

    /// Get or create a buffer pool for a specific source
    #[instrument(skip(self))]
    pub async fn get_pool(&self, source_id: &str) -> Arc<BufferPool> {
        let mut pools = self.pools.lock().await;

        if let Some(pool) = pools.get(source_id) {
            Arc::clone(pool)
        } else {
            debug!(source_id, "Creating new buffer pool for source");

            let pool = Arc::new(BufferPool::new(self.default_config.clone()));
            pools.insert(source_id.to_string(), Arc::clone(&pool));

            pool
        }
    }

    /// Cleanup old buffers in all pools
    #[instrument(skip(self))]
    pub async fn cleanup_all_pools(&self) {
        let pools = self.pools.lock().await;

        for (source_id, pool) in pools.iter() {
            debug!(source_id = %source_id, "Cleaning up buffer pool");
            pool.cleanup_old_buffers().await;
        }

        debug!(pool_count = pools.len(), "Buffer pool cleanup completed");
    }

    /// Get combined statistics from all pools
    pub async fn get_combined_stats(&self) -> BufferPoolInfo {
        let pools = self.pools.lock().await;
        let mut combined = BufferPoolInfo {
            small_buffers: 0,
            medium_buffers: 0,
            large_buffers: 0,
            total_buffers: 0,
            total_allocations: 0,
            total_recycled: 0,
            peak_pool_size: 0,
            total_memory_bytes: 0,
            recycling_efficiency: 0.0,
        };

        for pool in pools.values() {
            let info = pool.get_pool_info().await;
            combined.small_buffers += info.small_buffers;
            combined.medium_buffers += info.medium_buffers;
            combined.large_buffers += info.large_buffers;
            combined.total_buffers += info.total_buffers;
            combined.total_allocations += info.total_allocations;
            combined.total_recycled += info.total_recycled;
            combined.peak_pool_size = combined.peak_pool_size.max(info.peak_pool_size);
            combined.total_memory_bytes += info.total_memory_bytes;
        }

        // Recalculate efficiency
        combined.recycling_efficiency = if combined.total_allocations == 0 {
            0.0
        } else {
            (combined.total_recycled as f64 / combined.total_allocations as f64) * 100.0
        };

        combined
    }

    /// Remove pools for inactive sources
    pub async fn remove_pool(&self, source_id: &str) {
        let mut pools = self.pools.lock().await;
        if pools.remove(source_id).is_some() {
            info!(source_id, "Removed buffer pool for inactive source");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_metadata_creation() {
        let metadata = FrameMetadata::new(42, 1024, 85, "test_source".to_string());

        assert_eq!(metadata.sequence, 42);
        assert_eq!(metadata.size, 1024);
        assert_eq!(metadata.quality, 85);
        assert_eq!(metadata.source_id, "test_source");
        assert!(metadata.age() < Duration::from_millis(10));
    }

    #[test]
    fn test_frame_buffer_creation() {
        let data = Bytes::from_static(b"\xFF\xD8test_jpeg_data\xFF\xD9");
        let metadata = FrameMetadata::new(1, data.len(), 85, "test".to_string());

        let frame = FrameBuffer::new(data.clone(), metadata);

        assert_eq!(frame.size(), data.len());
        assert!(frame.is_valid_jpeg());
        assert_eq!(frame.as_bytes(), data);
        assert!(frame.ref_count() >= 1);
    }

    #[test]
    fn test_buffer_pool_config_default() {
        let config = BufferPoolConfig::default();

        assert_eq!(config.max_buffers, 20);
        assert_eq!(config.small_buffer_size, 64 * 1024);
        assert_eq!(config.medium_buffer_size, 192 * 1024);
        assert_eq!(config.large_buffer_size, 384 * 1024);
        assert_eq!(config.max_buffer_age, Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_buffer_pool_allocation() {
        let config = BufferPoolConfig::default();
        let pool = BufferPool::new(config.clone());

        // Test small buffer allocation
        let small_buffer = pool.get_buffer(1024).await;
        assert_eq!(small_buffer.len(), 1024);

        // Test medium buffer allocation
        let medium_buffer = pool.get_buffer(100 * 1024).await;
        assert_eq!(medium_buffer.len(), 100 * 1024);

        // Test large buffer allocation
        let large_buffer = pool.get_buffer(500 * 1024).await;
        assert_eq!(large_buffer.len(), 500 * 1024);

        // Check stats
        let stats = pool.stats();
        assert_eq!(stats.total_allocations.load(Ordering::Relaxed), 3);
        assert_eq!(stats.total_recycled.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_buffer_recycling() {
        let config = BufferPoolConfig {
            max_buffers: 5,
            small_buffer_size: 1024,
            ..Default::default()
        };
        let pool = BufferPool::new(config);

        // Allocate and return a buffer
        let buffer1 = pool.get_buffer(512).await;
        assert_eq!(buffer1.len(), 512);

        pool.return_buffer(buffer1).await;

        // Allocate again - should reuse
        let buffer2 = pool.get_buffer(256).await;
        assert_eq!(buffer2.len(), 256);

        let stats = pool.stats();
        assert_eq!(stats.total_allocations.load(Ordering::Relaxed), 1);
        assert_eq!(stats.total_recycled.load(Ordering::Relaxed), 1);
        assert!(pool.recycling_efficiency() > 0.0);
    }

    #[tokio::test]
    async fn test_zero_copy_frame_sharing() {
        let data = Bytes::from(vec![0xFF, 0xD8, 1, 2, 3, 4, 5, 0xFF, 0xD9]);
        let metadata = FrameMetadata::new(1, data.len(), 85, "test".to_string());

        let frame1 = FrameBuffer::new(data.clone(), metadata);
        let frame2 = frame1.clone(); // Should be zero-copy

        // Both frames should share the same underlying data
        assert_eq!(frame1.ref_count(), frame2.ref_count());
        assert_eq!(frame1.as_bytes(), frame2.as_bytes());

        // Data should be identical
        assert_eq!(frame1.data.as_ptr(), frame2.data.as_ptr());
    }

    #[tokio::test]
    async fn test_buffer_pool_manager() {
        let config = BufferPoolConfig::default();
        let manager = BufferPoolManager::new(config);

        // Get pools for different sources
        let pool1 = manager.get_pool("source1").await;
        let pool2 = manager.get_pool("source2").await;
        let pool1_again = manager.get_pool("source1").await;

        // Should reuse the same pool for same source
        assert!(Arc::ptr_eq(&pool1, &pool1_again));

        // Different sources should have different pools
        assert!(!Arc::ptr_eq(&pool1, &pool2));

        // Test cleanup
        manager.cleanup_all_pools().await;

        // Test combined stats
        let stats = manager.get_combined_stats().await;
        assert_eq!(stats.total_buffers, 0); // No buffers allocated yet
    }

    #[test]
    fn test_heap_allocator() {
        let allocator = HeapAllocator;
        let buffer = allocator.allocate(1024);

        assert_eq!(buffer.len(), 1024);
        assert_eq!(allocator.name(), "heap");
    }
}
