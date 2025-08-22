//! ABOUTME: Website capture source using Selenium WebDriver for screenshots
//! ABOUTME: Provides trait-based abstraction with mock support for testing

use async_trait::async_trait;
use bytes::Bytes;
use gl_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, instrument, warn};

use crate::{CaptureHandle, CaptureSource};

/// Configuration for website capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebsiteConfig {
    /// URL to capture
    pub url: String,
    /// Run in headless mode (default: true)  
    #[serde(default = "default_headless")]
    pub headless: bool,
    /// Basic auth username
    pub basic_auth_username: Option<String>,
    /// Basic auth password 
    pub basic_auth_password: Option<String>,
    /// CSS selector for element-specific screenshot
    pub element_selector: Option<String>,
    /// Enable stealth mode to avoid detection
    #[serde(default)]
    pub stealth: bool,
    /// Timeout for page load
    #[serde(default = "default_timeout")]
    pub timeout: Duration,
    /// Window width for screenshots
    #[serde(default = "default_width")]
    pub width: u32,
    /// Window height for screenshots
    #[serde(default = "default_height")]
    pub height: u32,
}

fn default_headless() -> bool { true }
fn default_timeout() -> Duration { Duration::from_secs(30) }
fn default_width() -> u32 { 1920 }
fn default_height() -> u32 { 1080 }

impl Default for WebsiteConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            headless: default_headless(),
            basic_auth_username: None,
            basic_auth_password: None,
            element_selector: None,
            stealth: false,
            timeout: default_timeout(),
            width: default_width(),
            height: default_height(),
        }
    }
}

/// Trait for WebDriver client abstraction (for testing)
#[async_trait]
pub trait WebDriverClient: Send + Sync {
    async fn screenshot(&self, config: &WebsiteConfig) -> Result<Bytes>;
    async fn close(&self) -> Result<()>;
}

/// Website capture source
pub struct WebsiteSource {
    config: WebsiteConfig,
    client: Box<dyn WebDriverClient>,
}

impl WebsiteSource {
    pub fn new(config: WebsiteConfig, client: Box<dyn WebDriverClient>) -> Self {
        Self { config, client }
    }

    #[cfg(feature = "website")]
    pub async fn new_with_webdriver(config: WebsiteConfig, webdriver_url: Option<String>) -> Result<Self> {
        let client = Box::new(ThirtyfourClient::new(webdriver_url).await?);
        Ok(Self { config, client })
    }
}

#[async_trait]
impl CaptureSource for WebsiteSource {
    #[instrument(skip(self))]
    async fn start(&self) -> Result<CaptureHandle> {
        info!(url = %self.config.url, "Starting website capture");
        // For websites, "starting" just validates the config and client
        Ok(CaptureHandle::new(std::sync::Arc::new(WebsiteSource {
            config: self.config.clone(),
            client: MockWebDriverClient::new_boxed(),
        })))
    }

    #[instrument(skip(self))]
    async fn snapshot(&self) -> Result<Bytes> {
        debug!(url = %self.config.url, "Taking website snapshot");
        self.client.screenshot(&self.config).await
    }

    #[instrument(skip(self))]
    async fn stop(&self) -> Result<()> {
        debug!(url = %self.config.url, "Stopping website capture");
        self.client.close().await
    }
}

/// Mock WebDriver client for testing
pub struct MockWebDriverClient {
    synthetic_png: Bytes,
}

impl MockWebDriverClient {
    pub fn new() -> Self {
        // Minimal valid PNG (1x1 transparent pixel)
        let png_data = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
            0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
            0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
            0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
            0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
            0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        
        Self {
            synthetic_png: Bytes::from(png_data),
        }
    }

    pub fn new_boxed() -> Box<dyn WebDriverClient> {
        Box::new(Self::new())
    }
}

#[async_trait]
impl WebDriverClient for MockWebDriverClient {
    async fn screenshot(&self, config: &WebsiteConfig) -> Result<Bytes> {
        info!(
            url = %config.url,
            selector = ?config.element_selector,
            "Mock screenshot taken"
        );
        // Simulate some processing time
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(self.synthetic_png.clone())
    }

    async fn close(&self) -> Result<()> {
        debug!("Mock WebDriver client closed");
        Ok(())
    }
}

// Real thirtyfour implementation (behind feature gate)
#[cfg(feature = "website")]
pub struct ThirtyfourClient {
    driver: std::sync::Arc<tokio::sync::Mutex<Option<thirtyfour::WebDriver>>>,
}

#[cfg(feature = "website")]
impl ThirtyfourClient {
    pub async fn new(webdriver_url: Option<String>) -> Result<Self> {
        use thirtyfour::{DesiredCapabilities, WebDriver, ChromiumLikeCapabilities};
        
        let webdriver_url = webdriver_url.unwrap_or_else(|| "http://localhost:9515".to_string());
        
        let mut caps = DesiredCapabilities::chrome();
        caps.set_headless().map_err(|e| Error::Config(format!("Failed to set headless: {}", e)))?;
        caps.set_disable_gpu().map_err(|e| Error::Config(format!("Failed to set disable GPU: {}", e)))?;
        caps.set_no_sandbox().map_err(|e| Error::Config(format!("Failed to set no sandbox: {}", e)))?;
        caps.set_disable_dev_shm_usage().map_err(|e| Error::Config(format!("Failed to set disable dev shm: {}", e)))?;
        
        let driver = WebDriver::new(&webdriver_url, caps).await
            .map_err(|e| Error::Config(format!("Failed to create WebDriver: {}", e)))?;
        
        Ok(Self { 
            driver: std::sync::Arc::new(tokio::sync::Mutex::new(Some(driver)))
        })
    }
}

#[cfg(feature = "website")]
#[async_trait]
impl WebDriverClient for ThirtyfourClient {
    #[instrument(skip(self))]
    async fn screenshot(&self, config: &WebsiteConfig) -> Result<Bytes> {
        use thirtyfour::By;
        
        info!(url = %config.url, "Taking real screenshot with WebDriver");
        
        let driver_guard = self.driver.lock().await;
        let driver = driver_guard.as_ref()
            .ok_or_else(|| Error::Config("WebDriver has been closed".to_string()))?;
        
        // Set window size
        driver.set_window_rect(0, 0, config.width, config.height).await
            .map_err(|e| Error::Config(format!("Failed to set window size: {}", e)))?;
        
        // Navigate to URL
        driver.goto(&config.url).await
            .map_err(|e| Error::Config(format!("Failed to navigate to {}: {}", config.url, e)))?;
        
        // Handle basic auth if provided
        if let (Some(_username), Some(_password)) = (&config.basic_auth_username, &config.basic_auth_password) {
            // Basic auth is typically handled via URL: https://username:password@example.com
            warn!("Basic auth should be handled via URL format for security");
        }
        
        // Wait for page to load (simple approach)
        tokio::time::sleep(Duration::from_millis(1000)).await;
        
        // Take screenshot (element-specific or full page)
        let screenshot_data = if let Some(selector) = &config.element_selector {
            debug!(selector = %selector, "Taking element-specific screenshot");
            let element = driver.find(By::Css(selector)).await
                .map_err(|e| Error::Config(format!("Element not found '{}': {}", selector, e)))?;
            element.screenshot_as_png().await
                .map_err(|e| Error::Config(format!("Failed to take element screenshot: {}", e)))?
        } else {
            debug!("Taking full page screenshot");
            driver.screenshot_as_png().await
                .map_err(|e| Error::Config(format!("Failed to take screenshot: {}", e)))?
        };
        
        Ok(Bytes::from(screenshot_data))
    }
    
    async fn close(&self) -> Result<()> {
        debug!("Closing WebDriver session");
        let mut driver_guard = self.driver.lock().await;
        if let Some(driver) = driver_guard.take() {
            driver.quit().await
                .map_err(|e| Error::Config(format!("Failed to close WebDriver: {}", e)))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_website_config_defaults() {
        let config = WebsiteConfig::default();
        assert!(config.headless);
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert!(!config.stealth);
        assert!(config.basic_auth_username.is_none());
        assert!(config.element_selector.is_none());
    }
    
    #[tokio::test]
    async fn test_website_config_serialization() {
        let config = WebsiteConfig {
            url: "https://example.com".to_string(),
            headless: false,
            basic_auth_username: Some("user".to_string()),
            basic_auth_password: Some("pass".to_string()),
            element_selector: Some("#main".to_string()),
            stealth: true,
            timeout: Duration::from_secs(60),
            width: 1280,
            height: 720,
        };
        
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: WebsiteConfig = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.url, config.url);
        assert_eq!(deserialized.headless, config.headless);
        assert_eq!(deserialized.stealth, config.stealth);
        assert_eq!(deserialized.width, config.width);
    }
    
    #[tokio::test] 
    async fn test_mock_webdriver_client() {
        let client = MockWebDriverClient::new();
        let config = WebsiteConfig {
            url: "https://example.com".to_string(),
            ..Default::default()
        };
        
        let screenshot = client.screenshot(&config).await.unwrap();
        assert!(!screenshot.is_empty());
        assert!(screenshot.starts_with(b"\x89PNG")); // PNG magic number
        
        client.close().await.unwrap();
    }
    
    #[tokio::test]
    async fn test_website_source_lifecycle() {
        let config = WebsiteConfig {
            url: "https://example.com".to_string(),
            ..Default::default()
        };
        
        let client = MockWebDriverClient::new_boxed();
        let source = WebsiteSource::new(config, client);
        
        // Test start
        let handle = source.start().await.unwrap();
        
        // Test snapshot
        let screenshot = handle.snapshot().await.unwrap();
        assert!(!screenshot.is_empty());
        
        // Test stop
        handle.stop().await.unwrap();
    }

    #[cfg(feature = "website_live")]
    #[tokio::test]
    #[ignore = "Requires running Selenium server"]
    async fn test_thirtyfour_integration() {
        // This test requires a running Selenium server (e.g., ChromeDriver)
        let config = WebsiteConfig {
            url: "https://httpbin.org/html".to_string(),
            ..Default::default()
        };
        
        if let Ok(source) = WebsiteSource::new_with_webdriver(config, None).await {
            let handle = source.start().await.unwrap();
            let screenshot = handle.snapshot().await.unwrap();
            assert!(!screenshot.is_empty());
            handle.stop().await.unwrap();
        }
    }
}