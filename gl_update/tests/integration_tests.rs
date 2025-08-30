//! Integration tests for the update system

use bytes::Bytes;
use gl_update::{
    signature::signing::*, GitHubReleaseChecker, HealthChecker, SignatureVerifier, UpdateConfig,
    UpdateStrategyType,
};
use serde_json::json;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_complete_update_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let (private_key, public_key) = generate_keypair();

    // Create fake binary data
    let fake_binary = b"fake binary content for testing";
    let signature = sign_data(fake_binary, &private_key).unwrap();

    // Setup mock GitHub server
    let github_server = MockServer::start().await;

    let release_response = json!({
        "id": 12345,
        "tag_name": "v1.2.0",
        "name": "Test Release 1.2.0",
        "body": "Test release for integration testing",
        "draft": false,
        "prerelease": false,
        "published_at": "2023-12-01T10:00:00Z",
        "assets": [
            {
                "id": 67890,
                "name": "glimpser-linux-x64",
                "label": null,
                "size": fake_binary.len(),
                "download_count": 0,
                "browser_download_url": format!("{}/download/glimpser-linux-x64", github_server.uri()),
                "content_type": "application/octet-stream"
            },
            {
                "id": 67891,
                "name": "glimpser-linux-x64.sig",
                "label": null,
                "size": signature.len(),
                "download_count": 0,
                "browser_download_url": format!("{}/download/glimpser-linux-x64.sig", github_server.uri()),
                "content_type": "text/plain"
            }
        ]
    });

    // Mock GitHub API endpoints
    Mock::given(method("GET"))
        .and(path("/repos/test/repo/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&release_response))
        .mount(&github_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download/glimpser-linux-x64"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fake_binary))
        .mount(&github_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/download/glimpser-linux-x64.sig"))
        .respond_with(ResponseTemplate::new(200).set_body_string(&signature))
        .mount(&github_server)
        .await;

    // Setup mock health server
    let health_server = MockServer::start().await;

    let health_response = json!({
        "status": "ok",
        "version": "1.2.0"
    });

    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&health_response))
        .mount(&health_server)
        .await;

    // Create update configuration
    let config = UpdateConfig {
        repository: "test/repo".to_string(),
        current_version: "1.1.0".to_string(),
        public_key: public_key,
        strategy: UpdateStrategyType::Sidecar,
        health_check_url: format!("{}/healthz", health_server.uri()),
        health_check_timeout_seconds: 5,
        binary_name: "glimpser".to_string(),
        install_dir: temp_dir.path().to_path_buf(),
        auto_apply: false,
        github_token: None,
        check_interval_seconds: 3600,
    };

    // Create update service with custom GitHub URL
    let release_checker =
        GitHubReleaseChecker::new(config.repository.clone(), config.github_token.clone())
            .with_base_url(github_server.uri());

    let signature_verifier = SignatureVerifier::new(&config.public_key).unwrap();
    let health_checker = HealthChecker::new(
        config.health_check_url.clone(),
        Duration::from_secs(config.health_check_timeout_seconds),
    );

    // Test 1: Check for updates
    let latest_release = release_checker.get_latest_release().await.unwrap();
    assert_eq!(latest_release.tag_name, "v1.2.0");
    assert_eq!(latest_release.assets.len(), 2);

    // Test 2: Download and verify signature
    let binary_asset = latest_release
        .assets
        .iter()
        .find(|a| a.name == "glimpser-linux-x64")
        .unwrap();

    let sig_asset = latest_release
        .assets
        .iter()
        .find(|a| a.name == "glimpser-linux-x64.sig")
        .unwrap();

    let binary_data = release_checker.download_asset(binary_asset).await.unwrap();
    let sig_data = release_checker.download_asset(sig_asset).await.unwrap();
    let sig_string = String::from_utf8(sig_data.to_vec())
        .unwrap()
        .trim()
        .to_string();

    assert_eq!(binary_data.as_ref(), fake_binary);

    // Test signature verification
    let verify_result = signature_verifier.verify(&binary_data, &sig_string);
    assert!(
        verify_result.is_ok(),
        "Signature verification failed: {:?}",
        verify_result
    );

    // Test 3: Health check
    let health_result = health_checker.check_health().await;
    assert!(
        health_result.is_ok(),
        "Health check failed: {:?}",
        health_result
    );

    // Test 4: Version-specific health check
    let version_result = health_checker.check_version("1.2.0").await;
    assert!(
        version_result.is_ok(),
        "Version check failed: {:?}",
        version_result
    );
}

#[tokio::test]
async fn test_signature_verification_failure() {
    let _temp_dir = TempDir::new().unwrap();
    let (private_key1, public_key1) = generate_keypair();
    let (private_key2, _) = generate_keypair(); // Different key pair

    let test_data = b"test data for signature verification";

    // Sign with one key
    let signature = sign_data(test_data, &private_key1).unwrap();

    // Try to verify with different public key
    let verifier = SignatureVerifier::new(&public_key1).unwrap();
    let result = verifier.verify(&Bytes::from(test_data.to_vec()), &signature);
    assert!(result.is_ok());

    // Now sign with different private key
    let wrong_signature = sign_data(test_data, &private_key2).unwrap();

    // Should fail verification
    let result = verifier.verify(&Bytes::from(test_data.to_vec()), &wrong_signature);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("verification failed"));
}

#[tokio::test]
async fn test_github_api_error_handling() {
    let github_server = MockServer::start().await;

    // Mock 404 response
    Mock::given(method("GET"))
        .and(path("/repos/test/repo/releases/latest"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&github_server)
        .await;

    let checker =
        GitHubReleaseChecker::new("test/repo".to_string(), None).with_base_url(github_server.uri());

    let result = checker.get_latest_release().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("404"));
}

#[tokio::test]
async fn test_health_check_retry_logic() {
    let health_server = MockServer::start().await;

    // First two calls fail, third succeeds
    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .mount(&health_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"status": "ok"})))
        .mount(&health_server)
        .await;

    let checker = HealthChecker::new(
        format!("{}/healthz", health_server.uri()),
        Duration::from_secs(5),
    )
    .with_retries(3, Duration::from_millis(100));

    let result = checker.check_health().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_health_check_timeout() {
    let health_server = MockServer::start().await;

    // Mock a slow response that exceeds timeout
    Mock::given(method("GET"))
        .and(path("/healthz"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"status": "ok"}))
                .set_delay(Duration::from_secs(10)), // Longer than timeout
        )
        .mount(&health_server)
        .await;

    let checker = HealthChecker::new(
        format!("{}/healthz", health_server.uri()),
        Duration::from_secs(1), // Short timeout
    )
    .with_retries(1, Duration::from_millis(100));

    let result = checker.check_health().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("request failed"));
}

#[tokio::test]
async fn test_no_update_available() {
    let github_server = MockServer::start().await;

    let release_response = json!({
        "id": 12345,
        "tag_name": "v1.0.0", // Same as current version
        "name": "Current Release",
        "body": "Current release",
        "draft": false,
        "prerelease": false,
        "published_at": "2023-11-01T10:00:00Z",
        "assets": []
    });

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&release_response))
        .mount(&github_server)
        .await;

    let checker =
        GitHubReleaseChecker::new("test/repo".to_string(), None).with_base_url(github_server.uri());

    let release = checker.get_latest_release().await.unwrap();

    // Simulate the version comparison logic
    let current_version = "v1.0.0";
    let is_newer =
        release.tag_name.trim_start_matches('v') > current_version.trim_start_matches('v');

    assert!(!is_newer);
}

#[test]
fn test_signature_key_generation() {
    let (private_key, public_key) = generate_keypair();

    // Keys should be valid hex strings
    assert!(hex::decode(&private_key).is_ok());
    assert!(hex::decode(&public_key).is_ok());

    // Should be correct lengths
    assert_eq!(private_key.len(), 64); // 32 bytes * 2
    assert_eq!(public_key.len(), 64); // 32 bytes * 2

    // Should be able to create verifier with generated public key
    let verifier = SignatureVerifier::new(&public_key);
    assert!(verifier.is_ok());

    // Should be able to sign and verify
    let test_data = b"test signing data";
    let signature = sign_data(test_data, &private_key).unwrap();
    let verify_result = verifier
        .unwrap()
        .verify(&Bytes::from(test_data.to_vec()), &signature);
    assert!(verify_result.is_ok());
}

#[tokio::test]
async fn test_missing_signature_file() {
    let github_server = MockServer::start().await;

    let release_response = json!({
        "id": 12345,
        "tag_name": "v1.2.0",
        "name": "Release without signature",
        "body": "Test release missing signature file",
        "draft": false,
        "prerelease": false,
        "published_at": "2023-12-01T10:00:00Z",
        "assets": [
            {
                "id": 67890,
                "name": "glimpser-linux-x64",
                "label": null,
                "size": 1024,
                "download_count": 0,
                "browser_download_url": format!("{}/download/glimpser-linux-x64", github_server.uri()),
                "content_type": "application/octet-stream"
            }
            // Note: Missing signature file
        ]
    });

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&release_response))
        .mount(&github_server)
        .await;

    let checker =
        GitHubReleaseChecker::new("test/repo".to_string(), None).with_base_url(github_server.uri());

    let release = checker.get_latest_release().await.unwrap();

    // Should not find signature asset
    let binary_asset = release
        .assets
        .iter()
        .find(|a| a.name.contains("glimpser"))
        .unwrap();

    let signature_asset = release
        .assets
        .iter()
        .find(|a| a.name == format!("{}.sig", binary_asset.name));

    assert!(signature_asset.is_none());
}

#[tokio::test]
async fn test_draft_release_handling() {
    let github_server = MockServer::start().await;

    let release_response = json!({
        "id": 12345,
        "tag_name": "v1.2.0",
        "name": "Draft Release",
        "body": "This is a draft release",
        "draft": true, // Draft release
        "prerelease": false,
        "published_at": "2023-12-01T10:00:00Z",
        "assets": []
    });

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/releases/latest"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&release_response))
        .mount(&github_server)
        .await;

    let checker =
        GitHubReleaseChecker::new("test/repo".to_string(), None).with_base_url(github_server.uri());

    let result = checker.get_latest_release().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("draft"));
}
