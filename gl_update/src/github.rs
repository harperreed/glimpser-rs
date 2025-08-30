//! ABOUTME: GitHub release API client for checking and downloading updates
//! ABOUTME: Handles GitHub API interactions with rate limiting and error handling

use bytes::Bytes;
use chrono::{DateTime, Utc};
use gl_core::Result;
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};

/// GitHub release information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub id: u64,
    pub tag_name: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub published_at: Option<DateTime<Utc>>,
    pub assets: Vec<GitHubAsset>,
}

/// GitHub release asset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubAsset {
    pub id: u64,
    pub name: String,
    pub label: Option<String>,
    pub size: u64,
    pub download_count: u64,
    pub browser_download_url: String,
    pub content_type: String,
}

/// GitHub API client for release checking
pub struct GitHubReleaseChecker {
    client: Client,
    repository: String,
    api_token: Option<String>,
    base_url: String,
}

impl GitHubReleaseChecker {
    /// Create a new GitHub release checker
    pub fn new(repository: String, api_token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("glimpser-updater/1.0")
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            repository,
            api_token,
            base_url: "https://api.github.com".to_string(),
        }
    }

    /// Create a new GitHub release checker with custom API URL
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Get the latest release from GitHub
    pub async fn get_latest_release(&self) -> Result<GitHubRelease> {
        let url = format!(
            "{}/repos/{}/releases/latest",
            self.base_url, self.repository
        );

        info!("Fetching latest release from: {}", url);

        let mut request = self.client.get(&url);

        // Add authentication if available
        if let Some(token) = &self.api_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| gl_core::Error::External(format!("GitHub API request failed: {}", e)))?;

        self.handle_rate_limit(&response).await?;

        if !response.status().is_success() {
            return Err(gl_core::Error::External(format!(
                "GitHub API returned status: {} - {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let release: GitHubRelease = response.json().await.map_err(|e| {
            gl_core::Error::External(format!("Failed to parse GitHub API response: {}", e))
        })?;

        debug!(
            "Found latest release: {} ({})",
            release.tag_name, release.id
        );

        // Skip drafts and prereleases by default
        if release.draft {
            return Err(gl_core::Error::External(
                "Latest release is a draft".to_string(),
            ));
        }

        if release.prerelease {
            warn!("Latest release is a prerelease: {}", release.tag_name);
        }

        Ok(release)
    }

    /// Get all releases (paginated)
    pub async fn get_releases(&self, page: u32, per_page: u32) -> Result<Vec<GitHubRelease>> {
        let url = format!(
            "{}/repos/{}/releases?page={}&per_page={}",
            self.base_url, self.repository, page, per_page
        );

        info!("Fetching releases from: {}", url);

        let mut request = self.client.get(&url);

        if let Some(token) = &self.api_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .await
            .map_err(|e| gl_core::Error::External(format!("GitHub API request failed: {}", e)))?;

        self.handle_rate_limit(&response).await?;

        if !response.status().is_success() {
            return Err(gl_core::Error::External(format!(
                "GitHub API returned status: {}",
                response.status()
            )));
        }

        let releases: Vec<GitHubRelease> = response.json().await.map_err(|e| {
            gl_core::Error::External(format!("Failed to parse GitHub API response: {}", e))
        })?;

        debug!("Found {} releases on page {}", releases.len(), page);

        Ok(releases)
    }

    /// Download a release asset
    pub async fn download_asset(&self, asset: &GitHubAsset) -> Result<Bytes> {
        info!("Downloading asset: {} ({} bytes)", asset.name, asset.size);

        let mut request = self.client.get(&asset.browser_download_url);

        if let Some(token) = &self.api_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await.map_err(|e| {
            gl_core::Error::External(format!("Asset download request failed: {}", e))
        })?;

        self.handle_rate_limit(&response).await?;

        if !response.status().is_success() {
            return Err(gl_core::Error::External(format!(
                "Asset download failed with status: {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| gl_core::Error::External(format!("Failed to read asset data: {}", e)))?;

        if bytes.len() as u64 != asset.size {
            warn!(
                "Downloaded size ({}) doesn't match expected size ({})",
                bytes.len(),
                asset.size
            );
        }

        info!("Successfully downloaded {} bytes", bytes.len());
        Ok(bytes)
    }

    /// Check GitHub API rate limiting
    async fn handle_rate_limit(&self, response: &Response) -> Result<()> {
        if let Some(remaining) = response
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok())
        {
            if remaining < 10 {
                warn!(
                    "GitHub API rate limit low: {} requests remaining",
                    remaining
                );
            }

            if remaining == 0 {
                if let Some(reset_time) = response
                    .headers()
                    .get("x-ratelimit-reset")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<i64>().ok())
                {
                    let reset_at = DateTime::from_timestamp(reset_time, 0);
                    return Err(gl_core::Error::External(format!(
                        "GitHub API rate limit exceeded. Resets at: {:?}",
                        reset_at
                    )));
                } else {
                    return Err(gl_core::Error::External(
                        "GitHub API rate limit exceeded".to_string(),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Get repository information
    pub fn repository(&self) -> &str {
        &self.repository
    }

    /// Check if API token is configured
    pub fn has_token(&self) -> bool {
        self.api_token.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_github_release_checker_creation() {
        let checker = GitHubReleaseChecker::new("owner/repo".to_string(), None);
        assert_eq!(checker.repository(), "owner/repo");
        assert!(!checker.has_token());

        let checker_with_token =
            GitHubReleaseChecker::new("owner/repo".to_string(), Some("token123".to_string()));
        assert!(checker_with_token.has_token());
    }

    #[tokio::test]
    async fn test_get_latest_release_success() {
        let mock_server = MockServer::start().await;

        let mock_release = serde_json::json!({
            "id": 12345,
            "tag_name": "v1.2.3",
            "name": "Release 1.2.3",
            "body": "Bug fixes and improvements",
            "draft": false,
            "prerelease": false,
            "published_at": "2023-12-01T10:00:00Z",
            "assets": [
                {
                    "id": 67890,
                    "name": "app-linux-x64",
                    "label": null,
                    "size": 1024000,
                    "download_count": 42,
                    "browser_download_url": "https://github.com/owner/repo/releases/download/v1.2.3/app-linux-x64",
                    "content_type": "application/octet-stream"
                }
            ]
        });

        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&mock_release))
            .mount(&mock_server)
            .await;

        let checker = GitHubReleaseChecker::new("owner/repo".to_string(), None)
            .with_base_url(mock_server.uri());

        let release = checker.get_latest_release().await.unwrap();
        assert_eq!(release.tag_name, "v1.2.3");
        assert_eq!(release.id, 12345);
        assert!(!release.draft);
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "app-linux-x64");
    }

    #[tokio::test]
    async fn test_get_latest_release_not_found() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/releases/latest"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
            .mount(&mock_server)
            .await;

        let checker = GitHubReleaseChecker::new("owner/repo".to_string(), None)
            .with_base_url(mock_server.uri());

        let result = checker.get_latest_release().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("GitHub API returned status: 404"));
    }

    #[tokio::test]
    async fn test_get_latest_release_draft() {
        let mock_server = MockServer::start().await;

        let mock_release = serde_json::json!({
            "id": 12345,
            "tag_name": "v1.2.3",
            "name": "Release 1.2.3",
            "body": "Bug fixes and improvements",
            "draft": true,
            "prerelease": false,
            "published_at": "2023-12-01T10:00:00Z",
            "assets": []
        });

        Mock::given(method("GET"))
            .and(path("/repos/owner/repo/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&mock_release))
            .mount(&mock_server)
            .await;

        let checker = GitHubReleaseChecker::new("owner/repo".to_string(), None)
            .with_base_url(mock_server.uri());

        let result = checker.get_latest_release().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Latest release is a draft"));
    }

    #[tokio::test]
    async fn test_download_asset() {
        let mock_server = MockServer::start().await;
        let test_data = b"binary data here";

        Mock::given(method("GET"))
            .and(path("/download/asset"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(test_data))
            .mount(&mock_server)
            .await;

        let asset = GitHubAsset {
            id: 123,
            name: "test-asset".to_string(),
            label: None,
            size: test_data.len() as u64,
            download_count: 1,
            browser_download_url: format!("{}/download/asset", mock_server.uri()),
            content_type: "application/octet-stream".to_string(),
        };

        let checker = GitHubReleaseChecker::new("owner/repo".to_string(), None);
        let downloaded = checker.download_asset(&asset).await.unwrap();
        assert_eq!(downloaded.as_ref(), test_data);
    }

    #[test]
    fn test_github_asset_serialization() {
        let asset = GitHubAsset {
            id: 123,
            name: "test-asset".to_string(),
            label: Some("Test Asset".to_string()),
            size: 1024,
            download_count: 42,
            browser_download_url: "https://example.com/download".to_string(),
            content_type: "application/octet-stream".to_string(),
        };

        let json = serde_json::to_string(&asset).unwrap();
        let deserialized: GitHubAsset = serde_json::from_str(&json).unwrap();
        assert_eq!(asset.name, deserialized.name);
        assert_eq!(asset.size, deserialized.size);
    }
}
