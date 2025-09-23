# MJPEG Performance Optimization Plan

## Executive Summary

This plan addresses critical performance bottlenecks in Glimpser's MJPEG streaming system. Current implementation spawns new FFmpeg processes for each frame, creating 100-500ms latency. The optimization strategy targets 5-10x performance improvement through persistent processes, hardware acceleration, and zero-copy streaming.

## Current Architecture Analysis

### Performance Bottlenecks Identified

```
Current Frame Pipeline:
[Video Source] -> [FFmpeg Process Spawn] -> [JPEG Generation] -> [Broadcast] -> [Client Streams]
     ~1ms             ~100-500ms              ~10-50ms           ~1-5ms        ~variable

Target Optimized Pipeline:
[Video Source] -> [Persistent FFmpeg] -> [Raw Frames] -> [Rust JPEG] -> [Zero-Copy Broadcast] -> [Clients]
     ~1ms             ~5-20ms            ~1-2ms        ~5-10ms         ~1ms                    ~stable
```

### Critical Issues

1. **FFmpeg Process Spawning**: Each frame creates new process (gl_capture/src/lib.rs:133-202)
2. **Static Stream Problem**: Single-frame extraction (-vframes 1) creates slideshow effect instead of motion
3. **Frontend Coupling**: Streams tightly coupled to UI, causing freezes when streams fail
4. **Frame Memory Copying**: Multiple data copies in streaming pipeline (gl_stream/src/mjpeg.rs:144-154)
5. **Motion Detection Overhead**: Full-resolution processing (gl_vision/src/lib.rs:247-272)
6. **Backpressure Handling**: Simple try_recv() causes frame drops (gl_stream/src/mjpeg.rs:132-181)

## Implementation Roadmap

### Phase 1: Foundation Layer (CRITICAL IMPACT)

#### 1.1 Persistent FFmpeg Process Architecture

**Priority**: Critical | **Expected Gain**: 5-10x latency reduction

**Files Modified**:
- `gl_capture/src/lib.rs` - Replace `generate_snapshot_with_ffmpeg`
- `gl_capture/src/ffmpeg_source.rs` - Add persistent process management
- `gl_capture/src/process_pool.rs` - NEW: FFmpeg process pool

**Implementation Steps**:

1. **Create FfmpegProcessPool**
   ```rust
   pub struct FfmpegProcessPool {
       processes: Arc<Mutex<Vec<FfmpegProcess>>>,
       config: FfmpegConfig,
       metrics: ProcessPoolMetrics,
   }
   ```

2. **Implement Continuous Frame Extraction**
   - Replace `-vframes 1` with continuous MJPEG stream: `ffmpeg -i input -f mjpeg -r 10 pipe:1`
   - Long-running FFmpeg processes with stdout pipe reading
   - Frame boundary detection for MJPEG streams
   - Asynchronous frame buffering with tokio

3. **Process Health Management**
   - Health checks every 30 seconds
   - Automatic restart on process failure
   - Graceful shutdown handling

4. **Update CaptureSource Trait**
   - Add streaming mode support
   - Maintain backward compatibility
   - Enhanced error handling

**Success Criteria**:
- Frame latency < 20ms (from 100-500ms)
- Zero process creation overhead
- Stable memory usage over 24+ hour runs
- Automatic process recovery

#### 1.2 Hardware Acceleration Integration

**Priority**: Critical | **Expected Gain**: 30-50% CPU reduction

**Files Modified**:
- `gl_capture/src/ffmpeg_source.rs` - Update command generation
- `gl_capture/src/lib.rs` - Add acceleration detection

**Implementation Steps**:

1. **Platform Detection & Capability Probing**
   ```rust
   pub enum HardwareAccel {
       Vaapi,      // Linux
       VideoToolbox, // macOS
       Nvenc,      // NVIDIA
       None,       // Fallback
   }
   ```

2. **Dynamic FFmpeg Argument Generation**
   - Linux: `-hwaccel vaapi -hwaccel_output_format vaapi`
   - macOS: `-hwaccel videotoolbox`
   - Windows: `-hwaccel dxva2` or `-hwaccel d3d11va`

3. **Graceful Fallback Mechanisms**
   - Test acceleration capability on startup
   - Automatic fallback on acceleration failure
   - Clear logging of acceleration status

**Success Criteria**:
- 30-50% CPU reduction when hardware acceleration available
- Seamless fallback when acceleration unavailable
- Platform-agnostic implementation

#### 1.3 Robust Viewing Pipeline & Frontend Isolation

**Priority**: Critical | **Expected Gain**: Eliminate frontend freezes, 100% uptime

**Problem Analysis**: Current streams cause UI freezes when FFmpeg fails or connections drop. Need complete isolation between stream health and frontend responsiveness.

**Files Modified**:
- `gl_web/src/routes/streams.rs` - Add robust streaming endpoints
- `gl_web/templates/stream_viewer.html` - NEW: Isolated stream viewer
- `gl_stream/src/viewer_service.rs` - NEW: Dedicated viewer service
- `gl_stream/src/stream_health.rs` - NEW: Health monitoring

**Implementation Steps**:

1. **Dedicated Stream Service**
   ```rust
   pub struct RobustStreamViewer {
       stream_manager: Arc<StreamManager>,
       health_monitor: HealthMonitor,
       viewer_count: Arc<AtomicUsize>,
       recovery_strategy: RecoveryStrategy,
   }
   ```

2. **Frontend Isolation Architecture**
   - WebSocket-based control channel separate from video stream
   - WebWorker for stream processing to isolate from main UI thread
   - Progressive degradation: placeholder images during recovery
   - Client-side buffering for smooth playback during gaps

3. **Multi-layered Resilience**
   ```rust
   pub enum StreamHealth {
       Healthy,
       Degraded { quality_reduced: bool },
       Recovering { retry_count: u32 },
       Failed { last_error: String },
   }

   pub struct RecoveryStrategy {
       exponential_backoff: BackoffConfig,
       quality_degradation: bool,
       frame_interpolation: bool,
       placeholder_frames: bool,
   }
   ```

4. **On-Demand Resource Management**
   - Start FFmpeg processes only when first viewer connects
   - Viewer counting with automatic shutdown when last client disconnects
   - Resource pooling to prevent duplicate processes for same source

5. **Stream Health Monitoring**
   - Real-time health status per stream
   - Automatic recovery with exponential backoff
   - Quality metrics and error reporting
   - Client notification of stream state changes

**Success Criteria**:
- Zero frontend freezes during stream failures
- < 2 second recovery time from stream errors
- Smooth degradation during network issues
- Resource usage scales with active viewers only

### Phase 2: Streaming Pipeline Optimization (HIGH IMPACT)

#### 2.1 Zero-Copy Frame Pipeline

**Priority**: High | **Expected Gain**: 2-3x throughput improvement

**Files Modified**:
- `gl_stream/src/mjpeg.rs` - Optimize buffer handling
- `gl_stream/src/lib.rs` - Update StreamSession frame management

**Implementation Steps**:

1. **Shared Buffer Pools**
   ```rust
   pub struct FrameBuffer {
       data: Arc<[u8]>,
       metadata: FrameMetadata,
   }

   pub struct BufferPool {
       buffers: Vec<FrameBuffer>,
       allocator: Box<dyn BufferAllocator>,
   }
   ```

2. **Reference-Counted Frame Data**
   - Eliminate Bytes::clone() operations
   - Implement Arc-based frame sharing
   - Pre-allocated multipart boundaries

3. **Optimized Broadcast Channel Usage**
   - Custom broadcast implementation for frames
   - Per-client frame queues with different priorities
   - Buffer pool integration

**Success Criteria**:
- 60-80% reduction in memory allocations
- Stable memory usage under concurrent load
- 2-3x improvement in frames/second throughput

#### 2.2 Enhanced Backpressure Management

**Priority**: High | **Expected Gain**: 2-3x client capacity

**Files Modified**:
- `gl_stream/src/mjpeg.rs` - Improve Stream::poll_next
- `gl_stream/src/lib.rs` - Add adaptive frame rate

**Implementation Steps**:

1. **Proper Async Polling**
   ```rust
   fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
       // Replace try_recv() with proper async waiting
       match Pin::new(&mut self.frame_receiver).poll_recv(cx) {
           Poll::Ready(Ok(frame)) => Poll::Ready(Some(Ok(self.format_frame(frame)))),
           Poll::Ready(Err(_)) => Poll::Ready(None),
           Poll::Pending => Poll::Pending,
       }
   }
   ```

2. **Client-Specific Frame Dropping**
   - Track per-client consumption rates
   - Drop frames for slow clients only
   - Maintain quality for fast clients

3. **Adaptive Frame Rate Logic**
   - Reduce frame rate when all clients are slow
   - Increase frame rate when clients catch up
   - Configurable adaptation parameters

**Success Criteria**:
- Support 2-3x more concurrent clients
- Smooth playback for fast clients regardless of slow clients
- Comprehensive metrics on client performance

### Phase 3: Motion Detection & Raw Frame Processing (MEDIUM IMPACT)

#### 3.1 Motion Detection Optimization

**Priority**: Medium | **Expected Gain**: 1.5-2x processing speed

**Files Modified**:
- `gl_vision/src/lib.rs` - Update default MotionConfig
- `gl_vision/src/pixel_detector.rs` - Add ROI support
- `gl_vision/src/lib.rs` - Implement adaptive thresholding

**Implementation Steps**:

1. **Parameter Optimization**
   ```rust
   impl Default for MotionConfig {
       fn default() -> Self {
           Self {
               downscale_factor: 6,  // Increased from 4
               max_width: 240,       // Reduced from 320
               max_height: 180,      // Reduced from 240
               threshold: 0.08,      // Reduced from 0.1
               min_change_area: 80,  // Reduced from 100
           }
       }
   }
   ```

2. **Region of Interest (ROI) Detection**
   - Configurable detection regions
   - Skip processing for non-ROI areas
   - Multiple ROI support per stream

3. **Adaptive Threshold Adjustment**
   - Scene complexity analysis
   - Automatic threshold tuning
   - Lighting condition adaptation

**Success Criteria**:
- 40-60% reduction in motion detection CPU usage
- Maintained or improved detection accuracy
- Configurable ROI support

#### 3.2 Raw Frame Processing Pipeline

**Priority**: Medium | **Expected Gain**: 20-30% encoding improvement

**Files Modified**:
- `gl_capture/src/lib.rs` - Add raw frame support
- `gl_stream/src/lib.rs` - Format conversion pipeline
- `gl_stream/src/frame_encoder.rs` - NEW: JPEG encoding

**Implementation Steps**:

1. **Raw Frame Extraction**
   ```bash
   ffmpeg -i input -f rawvideo -pix_fmt yuv420p pipe:1
   ```

2. **YUV to RGB Conversion**
   - Fast YUV420P to RGB24 conversion
   - SIMD optimizations where available
   - Memory-efficient processing

3. **Rust JPEG Encoding**
   - Use `image` crate for JPEG encoding
   - Configurable quality parameters
   - Quality vs speed optimization

**Success Criteria**:
- Better quality control than FFmpeg JPEG
- 20-30% encoding speed improvement
- Fallback to FFmpeg JPEG when needed

### Phase 4: Motion/Animation Enhancement (CRITICAL FOR UX)

#### 4.1 Continuous Motion Streaming

**Priority**: Critical | **Expected Gain**: Transform static slideshow to smooth video

**Root Cause**: Current `generate_snapshot_with_ffmpeg` uses `-vframes 1` which extracts exactly one frame per call, creating slideshow effect instead of continuous motion.

**Files Modified**:
- `gl_capture/src/lib.rs` - Replace single-frame logic
- `gl_capture/src/continuous_stream.rs` - NEW: Continuous stream parser
- `gl_stream/src/mjpeg_parser.rs` - NEW: MJPEG boundary detection

**Implementation Steps**:

1. **Continuous MJPEG Stream Generation**
   ```rust
   // Replace this:
   let args = vec!["-i", input, "-vframes", "1", "-f", "image2", "pipe:1"];

   // With this:
   let args = vec!["-i", input, "-f", "mjpeg", "-r", "10", "-q:v", "3", "pipe:1"];
   ```

2. **MJPEG Stream Parser**
   ```rust
   pub struct MjpegStreamParser {
       buffer: BytesMut,
       boundary: Option<String>,
       state: ParseState,
   }

   enum ParseState {
       SeekingBoundary,
       ReadingHeaders,
       ReadingFrame { content_length: usize },
   }
   ```

3. **Frame Boundary Detection**
   - Detect multipart boundaries in continuous stream
   - Parse Content-Length headers
   - Extract complete JPEG frames
   - Handle malformed or incomplete frames gracefully

4. **Smooth Playback Pipeline**
   - Buffer 3-5 frames for smooth playback
   - Frame interpolation during temporary gaps
   - Quality adaptation based on decode performance
   - Timestamp synchronization for proper frame rate

**Success Criteria**:
- Smooth motion instead of static slideshow
- Consistent frame rate (configurable, default 10fps)
- No frame drops during normal operation
- Graceful handling of source interruptions

#### 4.2 Advanced Animation Features

**Files Modified**:
- `gl_stream/src/animation_engine.rs` - NEW: Animation enhancement
- `gl_stream/src/frame_interpolation.rs` - NEW: Frame interpolation

**Implementation Steps**:

1. **Frame Interpolation**
   - Motion-based frame interpolation during gaps
   - Smooth transitions between keyframes
   - Configurable interpolation modes (linear, motion-vector)

2. **Quality-Based Animation**
   - Dynamic frame rate adjustment based on content complexity
   - Skip frames for static scenes to save bandwidth
   - Boost frame rate for motion-heavy scenes

3. **Client-Side Animation Enhancement**
   - WebGL-based smooth frame transitions
   - Predictive buffering for interactive streams
   - Touch/gesture controls for mobile viewing

**Success Criteria**:
- Smooth 60fps client-side animation even with 10fps source
- Intelligent quality adaptation
- Enhanced user experience on mobile devices

## Testing & Validation Strategy

### Performance Benchmarks

**Target Metrics**:
- Frame generation latency: < 20ms (currently 100-500ms)
- Motion streaming: Smooth 10fps continuous video (from static slideshow)
- Frontend isolation: Zero UI freezes during stream failures
- Memory usage: Stable under load, no leaks
- CPU utilization: 30-50% reduction with hardware acceleration
- Concurrent clients: 2-3x capacity increase
- Motion detection: 40-60% processing time reduction
- Stream recovery: < 2 seconds from failure to restored stream

### Testing Approach

1. **Unit Tests**
   - Process pool management
   - Buffer pool operations
   - Frame encoding/decoding
   - Motion detection accuracy
   - MJPEG stream parsing
   - Frontend isolation mechanisms

2. **Integration Tests**
   - End-to-end streaming with real video
   - Multi-client concurrent streaming
   - Hardware acceleration on different platforms
   - Long-running stability tests
   - Stream failure and recovery scenarios
   - Frontend freeze prevention validation

3. **Load Testing**
   - Concurrent client capacity limits
   - Memory usage under sustained load
   - Process restart recovery
   - Network bandwidth optimization
   - Frontend responsiveness under stream stress

4. **Motion/Animation Testing**
   - Frame rate consistency validation
   - Smooth motion vs static comparison
   - Stream interruption handling
   - Quality adaptation testing

5. **Benchmarking Tools**
   ```
   gl_stream/benches/
   ├── mjpeg_streaming.rs      # Comprehensive streaming scenarios
   ├── buffer_management.rs    # Memory allocation benchmarks
   ├── concurrent_clients.rs   # Multi-client load testing
   ├── motion_animation.rs     # Continuous motion vs static comparison
   └── frontend_isolation.rs   # UI responsiveness under stream stress

   gl_capture/benches/
   ├── ffmpeg_performance.rs   # Process vs persistent comparison
   ├── hardware_accel.rs       # Acceleration benchmark suite
   ├── continuous_stream.rs    # MJPEG parsing performance
   └── stream_recovery.rs      # Failure and recovery timing

   gl_vision/benches/
   └── motion_detection.rs     # ROI and threshold optimization
   ```

## Rollout & Deployment Strategy

### Phase 1: Core Infrastructure
```
Week 1-3: Persistent FFmpeg + Hardware Acceleration
├── Feature Flag: persistent_ffmpeg (default: false)
├── Development Environment Testing
├── Memory Leak & Stability Monitoring
└── Performance Baseline Comparison
```

### Phase 2: Streaming Optimizations
```
Week 4-5: Zero-Copy + Backpressure Management
├── Feature Flag: optimized_streaming (default: false)
├── A/B Testing with Stream Subset
├── Client Connection Quality Monitoring
└── Rollback Plan if Performance Degrades
```

### Phase 3: Motion & Processing
```
Week 6-7: Motion Detection + Raw Frame Processing
├── Feature Flag: enhanced_motion_detection (default: false)
├── Real-world Parameter Tuning
├── Motion Detection Accuracy QA
└── Multi-lighting Condition Validation
```

## Risk Mitigation

### Technical Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| FFmpeg process management complexity | High | Comprehensive testing, graceful degradation |
| Hardware acceleration platform differences | Medium | Robust fallback mechanisms |
| Memory leaks in persistent processes | High | Extensive leak testing, process recycling |
| Backward compatibility issues | Medium | Feature flags, parallel implementations |

### Operational Risks

| Risk | Impact | Mitigation |
|------|--------|------------|
| Performance regression | High | Comprehensive benchmarking, rollback procedures |
| Increased system complexity | Medium | Clear documentation, monitoring dashboards |
| Resource exhaustion | Medium | Resource limits, monitoring, auto-scaling |

## Success Metrics & Monitoring

### Key Performance Indicators

```
Performance Dashboard:
├── Frame Generation Latency (target: < 20ms)
├── System Resource Utilization (CPU, Memory, I/O)
├── Client Connection Success Rate & Stability
├── Motion Detection Accuracy & Speed
└── Overall System Throughput (concurrent streams)
```

### Monitoring Implementation

1. **Enhanced Metrics** (`gl_stream/src/metrics.rs`)
   - Frame generation timing
   - Buffer pool utilization
   - Client connection health
   - Hardware acceleration status

2. **Dashboards & Alerting**
   - Prometheus metrics collection
   - Grafana visualization dashboards
   - Alert rules for performance degradation
   - Resource exhaustion warnings

3. **Performance Reports**
   - Daily performance summaries
   - Weekly optimization opportunities
   - Monthly capacity planning reviews

## Implementation Priority Matrix

```
High Impact, High Effort:     │ High Impact, Low Effort:
├── Persistent FFmpeg         │ ├── Motion Config Tuning
├── Zero-Copy Pipeline        │ ├── Hardware Accel Detection
└── Backpressure Management   │ └── Buffer Pool Optimization

Low Impact, High Effort:      │ Low Impact, Low Effort:
├── Advanced Streaming        │ ├── Enhanced Logging
├── Custom JPEG Encoding      │ ├── Metrics Expansion
└── Complex ROI Detection     │ └── Documentation Updates
```

## Conclusion

This comprehensive optimization plan targets a 5-10x performance improvement in MJPEG streaming through systematic elimination of bottlenecks. The phased approach ensures stability while delivering measurable improvements at each stage. Success depends on thorough testing, proper monitoring, and gradual rollout with feature flags.

The implementation prioritizes high-impact changes first (persistent FFmpeg processes) while maintaining backward compatibility and operational safety through comprehensive testing and monitoring strategies.
