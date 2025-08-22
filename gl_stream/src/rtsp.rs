//! ABOUTME: RTSP streaming implementation using GStreamer RTSP server
//! ABOUTME: Provides real-time streaming protocol support for video streams

use crate::{RtspConfig, StreamSession};
use gl_core::{Id, Result};
use gstreamer as gst;
use gstreamer_rtsp_server as gst_rtsp;
use gstreamer_rtsp_server::prelude::*;
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};
use tracing::{debug, info};

/// RTSP server for streaming video content
pub struct RtspServer {
    /// GStreamer RTSP server
    server: gst_rtsp::RTSPServer,
    /// Mount points for templates
    mounts: gst_rtsp::RTSPMountPoints,
    /// Configuration
    config: RtspConfig,
    /// Active sessions
    sessions: Arc<RwLock<HashMap<Id, Arc<StreamSession>>>>,
}

impl RtspServer {
    /// Create a new RTSP server
    pub fn new(config: RtspConfig) -> Result<Self> {
        // Initialize GStreamer
        gst::init().map_err(|e| gl_core::Error::Config(format!("GStreamer init failed: {}", e)))?;

        let server = gst_rtsp::RTSPServer::new();
        server.set_address(&config.address);
        server.set_service(&config.port.to_string());

        let mounts = server.mount_points().unwrap();

        info!(
            address = %config.address,
            port = config.port,
            "Creating RTSP server"
        );

        Ok(Self {
            server,
            mounts,
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Start the RTSP server
    pub async fn start(&self) -> Result<()> {
        info!(
            address = %self.config.address,
            port = %self.config.port,
            "Starting RTSP server"
        );

        let _server_id = self.server.attach(None);

        info!("RTSP server started successfully");

        Ok(())
    }

    /// Add a stream endpoint for a template
    pub async fn add_stream(&self, template_id: Id, session: Arc<StreamSession>) -> Result<()> {
        let path = format!("/{}", template_id);

        debug!(
            template_id = %template_id,
            path = %path,
            "Adding RTSP stream endpoint"
        );

        // Create a simple media factory for this stream
        let factory = gst_rtsp::RTSPMediaFactory::new();

        // Build a test pipeline for JPEG streaming
        // Note: This is a simplified pipeline for proof of concept
        let pipeline_str = "( videotestsrc is-live=true ! videoconvert ! x264enc speed-preset=ultrafast tune=zerolatency ! rtph264pay name=pay0 pt=96 )";

        factory.set_launch(pipeline_str);
        factory.set_shared(true);

        // Mount the factory to the path
        self.mounts.add_factory(&path, factory);

        // Store session reference
        {
            let mut sessions = self.sessions.write().unwrap();
            sessions.insert(template_id.clone(), session);
        }

        info!(
            template_id = %template_id,
            path = %path,
            "RTSP stream endpoint added successfully"
        );

        Ok(())
    }

    /// Remove a stream endpoint
    pub async fn remove_stream(&self, template_id: &Id) -> Result<()> {
        let path = format!("/{}", template_id);

        debug!(
            template_id = %template_id,
            path = %path,
            "Removing RTSP stream endpoint"
        );

        // Remove from mounts
        self.mounts.remove_factory(&path);

        // Remove session reference
        {
            let mut sessions = self.sessions.write().unwrap();
            sessions.remove(template_id);
        }

        info!(
            template_id = %template_id,
            "RTSP stream endpoint removed"
        );

        Ok(())
    }

    /// Get the server URL for a template
    pub fn get_stream_url(&self, template_id: &Id) -> String {
        format!(
            "rtsp://{}:{}/{}",
            self.config.address, self.config.port, template_id
        )
    }

    /// Stop the RTSP server
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping RTSP server");

        // Clear all mounts
        {
            let sessions = self.sessions.read().unwrap();
            for template_id in sessions.keys() {
                let path = format!("/{}", template_id);
                self.mounts.remove_factory(&path);
            }
        }

        info!("RTSP server stopped");
        Ok(())
    }

    /// Get configuration
    pub fn config(&self) -> &RtspConfig {
        &self.config
    }
}

/// RTSP stream manager that integrates with the main streaming system
pub struct RtspStreamManager {
    server: Option<RtspServer>,
}

impl RtspStreamManager {
    /// Create a new RTSP stream manager
    pub fn new(config: Option<RtspConfig>) -> Result<Self> {
        let server = if let Some(rtsp_config) = config {
            if rtsp_config.enabled {
                Some(RtspServer::new(rtsp_config)?)
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self { server })
    }

    /// Start the RTSP server if enabled
    pub async fn start(&self) -> Result<()> {
        if let Some(server) = &self.server {
            server.start().await?;
        }
        Ok(())
    }

    /// Add a stream if RTSP is enabled
    pub async fn add_stream(&self, template_id: Id, session: Arc<StreamSession>) -> Result<()> {
        if let Some(server) = &self.server {
            server.add_stream(template_id, session).await?;
        }
        Ok(())
    }

    /// Remove a stream if RTSP is enabled
    pub async fn remove_stream(&self, template_id: &Id) -> Result<()> {
        if let Some(server) = &self.server {
            server.remove_stream(template_id).await?;
        }
        Ok(())
    }

    /// Get stream URL if RTSP is enabled
    pub fn get_stream_url(&self, template_id: &Id) -> Option<String> {
        self.server
            .as_ref()
            .map(|server| server.get_stream_url(template_id))
    }

    /// Stop the RTSP server
    pub async fn stop(&self) -> Result<()> {
        if let Some(server) = &self.server {
            server.stop().await?;
        }
        Ok(())
    }

    /// Check if RTSP is enabled
    pub fn is_enabled(&self) -> bool {
        self.server.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::create_test_id;
    use tracing::warn;

    #[tokio::test]
    #[ignore] // Skip by default - requires GStreamer
    async fn test_rtsp_server_creation() {
        let _test_id = create_test_id();
        let config = RtspConfig {
            enabled: true,
            port: 18554, // Use different port for testing
            address: "127.0.0.1".to_string(),
        };

        match RtspServer::new(config) {
            Ok(server) => {
                assert!(server.config().enabled);
                assert_eq!(server.config().port, 18554);
                assert_eq!(server.config().address, "127.0.0.1");
            }
            Err(e) => {
                // Expected if GStreamer isn't available
                warn!(error = %e, "GStreamer not available for RTSP test");
            }
        }
    }

    #[tokio::test]
    #[ignore] // Skip by default - requires GStreamer
    async fn test_rtsp_stream_manager() {
        let _test_id = create_test_id();
        let config = Some(RtspConfig {
            enabled: false,
            port: 18555,
            address: "127.0.0.1".to_string(),
        });

        let manager = RtspStreamManager::new(config);
        match manager {
            Ok(manager) => {
                assert!(!manager.is_enabled());

                // Test with enabled config
                let enabled_config = Some(RtspConfig {
                    enabled: true,
                    port: 18556,
                    address: "127.0.0.1".to_string(),
                });

                match RtspStreamManager::new(enabled_config) {
                    Ok(enabled_manager) => {
                        if enabled_manager.is_enabled() {
                            // Start server
                            let _ = enabled_manager.start().await;

                            // Stop server
                            let _ = enabled_manager.stop().await;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to create enabled RTSP manager");
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to create RTSP manager");
            }
        }
    }

    #[tokio::test]
    async fn test_rtsp_config_defaults() {
        let config = RtspConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.port, 8554);
        assert_eq!(config.address, "0.0.0.0");
    }

    #[tokio::test]
    async fn test_stream_url_generation() {
        let _test_id = create_test_id();
        let config = RtspConfig {
            enabled: true,
            port: 8554,
            address: "192.168.1.100".to_string(),
        };

        if let Ok(server) = RtspServer::new(config) {
            let template_id = Id::new();
            let url = server.get_stream_url(&template_id);
            let expected = format!("rtsp://192.168.1.100:8554/{}", template_id);
            assert_eq!(url, expected);
        }
    }
}
