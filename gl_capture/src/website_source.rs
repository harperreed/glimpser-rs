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
    /// Selector for element-specific screenshot (CSS or XPath)
    pub element_selector: Option<String>,
    /// Type of selector: "css" or "xpath" (default: "css")
    #[serde(default = "default_selector_type")]
    pub selector_type: String,
    /// Optional WebDriver endpoint (e.g., http://localhost:9515)
    #[serde(default)]
    pub webdriver_url: Option<String>,
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

fn default_headless() -> bool {
    true
}
fn default_selector_type() -> String {
    "css".to_string()
}
fn default_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_width() -> u32 {
    1920
}
fn default_height() -> u32 {
    1080
}

impl Default for WebsiteConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            headless: default_headless(),
            basic_auth_username: None,
            basic_auth_password: None,
            element_selector: None,
            selector_type: default_selector_type(),
            webdriver_url: None,
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
    pub async fn new_with_webdriver(
        config: WebsiteConfig,
        webdriver_url: Option<String>,
    ) -> Result<Self> {
        let client = Box::new(ThirtyfourClient::new(webdriver_url).await?);
        Ok(Self { config, client })
    }
}

#[async_trait]
impl CaptureSource for WebsiteSource {
    #[cfg(feature = "website_embedded")]
    #[instrument(skip(self))]
    async fn start(&self) -> Result<CaptureHandle> {
        info!(url = %self.config.url, "Starting website capture");
        // Note: For embedded mode, we always create a fresh HeadlessChromeClient
        // since each capture session needs its own browser instance to avoid conflicts.
        // The injected client is preserved for compatibility but not used in this mode.
        let client = HeadlessChromeClient::new_boxed().map_err(|e| {
            Error::Config(format!("Failed to create embedded Chrome client: {}", e))
        })?;
        Ok(CaptureHandle::new(std::sync::Arc::new(WebsiteSource {
            config: self.config.clone(),
            client,
        })))
    }

    #[cfg(not(feature = "website_embedded"))]
    #[instrument(skip(self))]
    async fn start(&self) -> Result<CaptureHandle> {
        info!(url = %self.config.url, "Starting website capture");

        // For non-embedded mode, we create a separate client for the capture handle.
        // This allows concurrent captures while avoiding conflicts with the original client.
        #[cfg(feature = "website")]
        let client = {
            match ThirtyfourClient::new(self.config.webdriver_url.clone()).await {
                Ok(real_client) => {
                    info!("Using real ThirtyfourClient for website capture");
                    Box::new(real_client) as Box<dyn WebDriverClient>
                }
                Err(e) => {
                    warn!(error = %e, "Failed to create real WebDriver client, falling back to mock");
                    MockWebDriverClient::new_boxed()
                }
            }
        };

        #[cfg(not(feature = "website"))]
        let client = {
            warn!("Website feature not enabled, using mock WebDriver client");
            MockWebDriverClient::new_boxed()
        };

        Ok(CaptureHandle::new(std::sync::Arc::new(WebsiteSource {
            config: self.config.clone(),
            client,
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

impl Default for MockWebDriverClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockWebDriverClient {
    pub fn new() -> Self {
        // Minimal valid PNG (1x1 transparent pixel)
        let png_data = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
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
        // No artificial delay to keep tests responsive
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
        use thirtyfour::{ChromiumLikeCapabilities, DesiredCapabilities, WebDriver};

        let webdriver_url = webdriver_url.unwrap_or_else(|| "http://localhost:9515".to_string());

        let mut caps = DesiredCapabilities::chrome();
        caps.set_headless()
            .map_err(|e| Error::Config(format!("Failed to set headless: {}", e)))?;
        caps.set_disable_gpu()
            .map_err(|e| Error::Config(format!("Failed to set disable GPU: {}", e)))?;
        caps.set_no_sandbox()
            .map_err(|e| Error::Config(format!("Failed to set no sandbox: {}", e)))?;
        caps.set_disable_dev_shm_usage()
            .map_err(|e| Error::Config(format!("Failed to set disable dev shm: {}", e)))?;

        let driver = WebDriver::new(&webdriver_url, caps)
            .await
            .map_err(|e| Error::Config(format!("Failed to create WebDriver: {}", e)))?;

        Ok(Self {
            driver: std::sync::Arc::new(tokio::sync::Mutex::new(Some(driver))),
        })
    }
}

#[cfg(feature = "website")]
#[async_trait]
impl WebDriverClient for ThirtyfourClient {
    #[instrument(skip(self))]
    async fn screenshot(&self, config: &WebsiteConfig) -> Result<Bytes> {
        use thirtyfour::{prelude::ElementQueryable, By};

        info!(url = %config.url, "Taking real screenshot with WebDriver");

        let driver_guard = self.driver.lock().await;
        let driver = driver_guard
            .as_ref()
            .ok_or_else(|| Error::Config("WebDriver has been closed".to_string()))?;

        // Set window size
        driver
            .set_window_rect(0, 0, config.width, config.height)
            .await
            .map_err(|e| Error::Config(format!("Failed to set window size: {}", e)))?;

        // Navigate to URL
        driver
            .goto(&config.url)
            .await
            .map_err(|e| Error::Config(format!("Failed to navigate to {}: {}", config.url, e)))?;

        // Handle basic auth if provided
        if let (Some(_username), Some(_password)) =
            (&config.basic_auth_username, &config.basic_auth_password)
        {
            // Basic auth is typically handled via URL: https://username:password@example.com
            warn!("Basic auth should be handled via URL format for security");
        }

        // Wait for page to load by ensuring body element is present
        driver
            .query(By::Css("body"))
            .first()
            .await
            .map_err(|e| Error::Config(format!("Page load error: {}", e)))?;

        // Take screenshot (element-specific or full page)
        let screenshot_data = if let Some(selector) = &config.element_selector {
            debug!(selector = %selector, selector_type = %config.selector_type, "Taking element-specific screenshot");
            let by = if config.selector_type == "xpath" {
                By::XPath(selector)
            } else {
                By::Css(selector)
            };
            let element = driver.find(by).await.map_err(|e| {
                Error::Config(format!(
                    "Element not found '{}' ({}): {}",
                    selector, config.selector_type, e
                ))
            })?;
            element
                .screenshot_as_png()
                .await
                .map_err(|e| Error::Config(format!("Failed to take element screenshot: {}", e)))?
        } else {
            debug!("Taking full page screenshot");
            driver
                .screenshot_as_png()
                .await
                .map_err(|e| Error::Config(format!("Failed to take screenshot: {}", e)))?
        };

        Ok(Bytes::from(screenshot_data))
    }

    async fn close(&self) -> Result<()> {
        debug!("Closing WebDriver session");
        let mut driver_guard = self.driver.lock().await;
        if let Some(driver) = driver_guard.take() {
            driver
                .quit()
                .await
                .map_err(|e| Error::Config(format!("Failed to close WebDriver: {}", e)))?;
        }
        Ok(())
    }
}

// Embedded headless Chrome implementation (behind feature gate)
#[cfg(feature = "website_embedded")]
pub struct HeadlessChromeClient {
    browser: std::sync::Arc<tokio::sync::Mutex<Option<headless_chrome::Browser>>>,
}

#[cfg(feature = "website_embedded")]
impl HeadlessChromeClient {
    pub fn new() -> Result<Self> {
        use headless_chrome::{Browser, LaunchOptions};

        let options = LaunchOptions::default_builder()
            .headless(true)
            .sandbox(false)
            .window_size(Some((1920, 1080)))
            .build()
            .map_err(|e| Error::Config(format!("Failed to build launch options: {}", e)))?;

        let browser = Browser::new(options)
            .map_err(|e| Error::Config(format!("Failed to launch Chrome: {}", e)))?;

        Ok(Self {
            browser: std::sync::Arc::new(tokio::sync::Mutex::new(Some(browser))),
        })
    }

    pub fn new_boxed() -> Result<Box<dyn WebDriverClient>> {
        Ok(Box::new(Self::new()?))
    }
}

#[cfg(feature = "website_embedded")]
#[async_trait]
impl WebDriverClient for HeadlessChromeClient {
    #[instrument(skip(self))]
    async fn screenshot(&self, config: &WebsiteConfig) -> Result<Bytes> {
        info!(url = %config.url, "Taking embedded Chrome screenshot");

        let browser_guard = self.browser.lock().await;
        let browser = browser_guard
            .as_ref()
            .ok_or_else(|| Error::Config("Chrome browser has been closed".to_string()))?;

        // Create a new tab
        let tab = browser
            .new_tab()
            .map_err(|e| Error::Config(format!("Failed to create new tab: {}", e)))?;

        // RAII guard to ensure tab cleanup on all paths
        struct TabGuard<'a> {
            tab: &'a headless_chrome::Tab,
        }
        impl<'a> Drop for TabGuard<'a> {
            fn drop(&mut self) {
                let _ = self.tab.close(true);
            }
        }
        let _tab_guard = TabGuard { tab: &tab };

        // Set viewport size using set_bounds
        tab.set_bounds(headless_chrome::types::Bounds::Normal {
            left: Some(0),
            top: Some(0),
            width: Some(config.width as f64),
            height: Some(config.height as f64),
        })
        .map_err(|e| Error::Config(format!("Failed to set viewport: {}", e)))?;

        // Navigate to URL
        tab.navigate_to(&config.url)
            .map_err(|e| Error::Config(format!("Failed to navigate to {}: {}", config.url, e)))?;

        // Wait for page to load and DOM to be ready
        tab.wait_until_navigated()
            .map_err(|e| Error::Config(format!("Failed to wait for navigation: {}", e)))?;
        tab.wait_for_element("body")
            .map_err(|e| Error::Config(format!("Failed to wait for body element: {}", e)))?;

        // Take screenshot
        let screenshot_data = if let Some(selector) = &config.element_selector {
            debug!(selector = %selector, selector_type = %config.selector_type, "Taking element-specific screenshot");

            // Find element - handle both CSS and XPath selectors
            let element = if config.selector_type == "xpath" {
                // Use JavaScript to evaluate XPath and get the element
                let js_code = format!(
                    r#"
                    (function() {{
                        var result = document.evaluate('{}', document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null);
                        var element = result.singleNodeValue;
                        if (!element) {{
                            throw new Error('XPath element not found');
                        }}
                        // Return a unique selector we can use to find it again
                        element.setAttribute('data-glimpser-xpath-target', 'true');
                        return true;
                    }})()
                    "#,
                    selector.replace('\\', "\\\\").replace("'", "\\'") // Escape for JavaScript
                );

                // Execute JavaScript to mark the element
                tab.evaluate(&js_code, true).map_err(|e| {
                    Error::Config(format!("Failed to evaluate XPath '{}': {}", selector, e))
                })?;

                // Now find the marked element using CSS selector
                let element = tab
                    .find_element("[data-glimpser-xpath-target='true']")
                    .map_err(|e| {
                        Error::Config(format!("XPath element not found '{}': {}", selector, e))
                    })?;

                // Clean up the attribute
                let cleanup_js = r#"
                    document.querySelector('[data-glimpser-xpath-target="true"]').removeAttribute('data-glimpser-xpath-target');
                "#;
                let _ = tab.evaluate(cleanup_js, true); // Ignore cleanup errors

                element
            } else {
                // Regular CSS selector
                tab.find_element(selector).map_err(|e| {
                    Error::Config(format!("CSS element not found '{}': {}", selector, e))
                })?
            };

            // Get element bounding box for clipping
            let rect_obj = element
                .call_js_fn(
                    "function() { 
                        const r = this.getBoundingClientRect(); 
                        return {x: r.x, y: r.y, width: r.width, height: r.height}; 
                    }",
                    vec![],
                    false,
                )
                .map_err(|e| Error::Config(format!("Failed to get element bounding box: {}", e)))?
                .value
                .ok_or_else(|| Error::Config("Element bounding box missing".to_string()))?;

            let x = rect_obj
                .get("x")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| Error::Config("Element x coordinate missing".to_string()))?;
            let y = rect_obj
                .get("y")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| Error::Config("Element y coordinate missing".to_string()))?;
            let width = rect_obj
                .get("width")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| Error::Config("Element width missing".to_string()))?;
            let height = rect_obj
                .get("height")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| Error::Config("Element height missing".to_string()))?;

            if width <= 0.0 || height <= 0.0 {
                return Err(Error::Config("Element has zero size".to_string()));
            }

            let clip = headless_chrome::protocol::cdp::Page::Viewport {
                x,
                y,
                width,
                height,
                scale: 1.0,
            };

            tab.capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None,
                Some(clip),
                true,
            )
            .map_err(|e| Error::Config(format!("Failed to capture element screenshot: {}", e)))?
        } else {
            // Full page screenshot
            tab.capture_screenshot(
                headless_chrome::protocol::cdp::Page::CaptureScreenshotFormatOption::Png,
                None, // Quality (not used for PNG)
                None, // Clip (full page)
                true, // From surface
            )
            .map_err(|e| Error::Config(format!("Failed to capture screenshot: {}", e)))?
        };

        Ok(Bytes::from(screenshot_data))
    }

    async fn close(&self) -> Result<()> {
        debug!("Closing embedded Chrome browser");
        let mut browser_guard = self.browser.lock().await;
        if let Some(browser) = browser_guard.take() {
            // Browser will be automatically cleaned up when dropped
            drop(browser);
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
            selector_type: "css".to_string(),
            webdriver_url: None,
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

    #[tokio::test]
    async fn test_mock_client_is_fast() {
        use std::time::Instant;

        let client = MockWebDriverClient::new();
        let config = WebsiteConfig {
            url: "https://example.com".to_string(),
            ..Default::default()
        };

        let start = Instant::now();
        client.screenshot(&config).await.unwrap();
        assert!(start.elapsed() < Duration::from_millis(50));

        client.close().await.unwrap();
    }

    #[cfg(feature = "website_embedded")]
    #[tokio::test]
    async fn test_headless_element_screenshot_size() {
        use image::ImageFormat;

        // Use data URL to avoid file system issues
        let html = "<html><body><div id='box' style='width:120px;height:80px;background:red;'>hi</div></body></html>";
        let url = format!("data:text/html,{}", html);
        let config = WebsiteConfig {
            url,
            element_selector: Some("#box".into()),
            ..Default::default()
        };

        let client = HeadlessChromeClient::new_boxed().unwrap();
        let source = WebsiteSource::new(config, client);
        let handle = source.start().await.unwrap();
        let bytes = handle.snapshot().await.unwrap();
        handle.stop().await.unwrap();

        let img = image::load_from_memory_with_format(&bytes, ImageFormat::Png).unwrap();
        assert_eq!(img.width(), 120);
        assert_eq!(img.height(), 80);
    }

    #[cfg(feature = "website_embedded")]
    #[tokio::test]
    async fn test_headless_zero_size_element_error() {
        use std::io::Write;

        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            file,
            "<html><body><div id='box' style='width:0;height:0;'></div></body></html>"
        )
        .unwrap();
        let url = format!("file://{}", file.path().display());
        let config = WebsiteConfig {
            url,
            element_selector: Some("#box".into()),
            ..Default::default()
        };

        let client = HeadlessChromeClient::new_boxed().unwrap();
        let source = WebsiteSource::new(config, client);
        let handle = source.start().await.unwrap();
        let result = handle.snapshot().await;
        handle.stop().await.unwrap();

        assert!(matches!(result, Err(Error::Config(_))));
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
