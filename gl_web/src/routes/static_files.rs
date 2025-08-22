//! ABOUTME: Static file serving for PWA with caching and CSP headers
//! ABOUTME: Provides SPA fallback routing and strong ETags for browser caching

use actix_files::{Files, NamedFile};
use actix_web::{
    dev::{fn_service, ServiceRequest, ServiceResponse},
    http::header::{
        CacheControl, CacheDirective, ETag, EntityTag, Header, HeaderName, HeaderValue,
        TryIntoHeaderValue,
    },
    middleware::DefaultHeaders,
    web, HttpRequest, HttpResponse, Result as ActixResult,
};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Configuration for static file serving
#[derive(Debug, Clone)]
pub struct StaticConfig {
    /// Directory containing static files
    pub static_dir: PathBuf,
    /// Maximum age for cache control (seconds)
    pub max_age: u32,
    /// Enable CSP headers
    pub enable_csp: bool,
    /// CSP nonce for inline scripts
    pub csp_nonce: Option<String>,
}

impl Default for StaticConfig {
    fn default() -> Self {
        Self {
            static_dir: PathBuf::from("./static"),
            max_age: 86400, // 1 day
            enable_csp: true,
            csp_nonce: None,
        }
    }
}

/// Generate CSP nonce
pub fn generate_csp_nonce() -> String {
    use rand_core::RngCore;
    let mut rng = rand_core::OsRng;
    let mut nonce_bytes = [0u8; 16];
    rng.fill_bytes(&mut nonce_bytes);
    hex::encode(nonce_bytes)
}

/// Generate ETag from file metadata
fn generate_etag(path: &Path) -> ActixResult<EntityTag> {
    let metadata =
        std::fs::metadata(path).map_err(|_| actix_web::error::ErrorNotFound("File not found"))?;

    let mtime = metadata
        .modified()
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let size = metadata.len();
    let etag_value = format!("{}-{}", mtime, size);

    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(etag_value.as_bytes());
    let etag_hash = hex::encode(&hash[..8]); // Use first 8 bytes for shorter ETag

    Ok(EntityTag::new_weak(etag_hash))
}

/// Static file handler with caching
#[actix_web::get("/{filename:.*}")]
pub async fn serve_static(
    path: web::Path<String>,
    req: HttpRequest,
    static_config: web::Data<StaticConfig>,
) -> ActixResult<HttpResponse> {
    let filename = path.into_inner();

    // Skip API and docs routes
    if filename.starts_with("api") || filename.starts_with("docs") {
        return Err(actix_web::error::ErrorNotFound("Not a static file"));
    }

    let full_path = static_config.static_dir.join(&filename);

    // Check if file exists, fallback to index.html for SPA routing
    let file_path = if full_path.exists() && full_path.is_file() {
        full_path
    } else {
        static_config.static_dir.join("index.html")
    };

    // Generate ETag
    let etag = generate_etag(&file_path)?;
    let etag_header_value = format!("W/\"{}\"", etag.tag());

    // Check If-None-Match header for 304 response
    if let Some(if_none_match) = req.headers().get("if-none-match") {
        if let Ok(if_none_match_str) = if_none_match.to_str() {
            if if_none_match_str.contains(&etag_header_value) {
                return Ok(HttpResponse::NotModified()
                    .insert_header(("etag", etag_header_value))
                    .finish());
            }
        }
    }

    // Serve the file with caching headers
    let file_content =
        std::fs::read(&file_path).map_err(|_| actix_web::error::ErrorNotFound("File not found"))?;

    let mut response = HttpResponse::Ok();

    // Add caching headers
    response.insert_header(("etag", etag_header_value));
    response.insert_header((
        "cache-control",
        format!("public, max-age={}", static_config.max_age),
    ));

    // Add CSP headers if enabled
    if static_config.enable_csp {
        let csp_value = if let Some(nonce) = &static_config.csp_nonce {
            format!(
                "default-src 'self'; script-src 'self' 'nonce-{}'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self' ws: wss:; frame-ancestors 'none';",
                nonce
            )
        } else {
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self'; connect-src 'self' ws: wss:; frame-ancestors 'none';".to_string()
        };

        response.insert_header(("content-security-policy", csp_value));
    }

    // Determine content type
    let content_type = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    Ok(response.content_type(content_type).body(file_content))
}

/// Create static file service with PWA support
pub fn create_static_service(_config: StaticConfig) -> actix_web::Scope {
    web::scope("").service(serve_static)
}

/// Middleware to add security headers
pub fn security_headers() -> DefaultHeaders {
    DefaultHeaders::new()
        .add(("X-Frame-Options", "DENY"))
        .add(("X-Content-Type-Options", "nosniff"))
        .add(("X-XSS-Protection", "1; mode=block"))
        .add(("Referrer-Policy", "strict-origin-when-cross-origin"))
        .add((
            "Strict-Transport-Security",
            "max-age=31536000; includeSubDomains",
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, web, App};
    use std::fs;
    use tempfile::TempDir;

    async fn create_test_static_dir() -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        // Create test files
        let index_content = r#"<!DOCTYPE html>
<html>
<head>
    <title>Test PWA</title>
</head>
<body>
    <div id="app">Test PWA Content</div>
</body>
</html>"#;

        fs::write(temp_dir.path().join("index.html"), index_content)
            .expect("Failed to write index.html");

        let js_content = "console.log('Test JavaScript');";
        fs::write(temp_dir.path().join("app.js"), js_content).expect("Failed to write app.js");

        temp_dir
    }

    #[actix_web::test]
    async fn test_static_file_serving() {
        let temp_dir = create_test_static_dir().await;
        let config = StaticConfig {
            static_dir: temp_dir.path().to_path_buf(),
            max_age: 3600,
            enable_csp: true,
            csp_nonce: Some("test123".to_string()),
        };

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(config.clone()))
                .service(create_static_service(config)),
        )
        .await;

        // Test serving index.html
        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());

        let body = test::read_body(resp).await;
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("Test PWA Content"));
    }

    #[actix_web::test]
    async fn test_spa_fallback_routing() {
        let temp_dir = create_test_static_dir().await;
        let config = StaticConfig {
            static_dir: temp_dir.path().to_path_buf(),
            max_age: 3600,
            enable_csp: true,
            csp_nonce: None,
        };

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(config.clone()))
                .service(create_static_service(config)),
        )
        .await;

        // Test SPA fallback - non-existent route should return index.html
        let req = test::TestRequest::get()
            .uri("/dashboard/settings")
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());

        let body = test::read_body(resp).await;
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("Test PWA Content"));
    }

    #[actix_web::test]
    async fn test_etag_caching() {
        let temp_dir = create_test_static_dir().await;
        let config = StaticConfig {
            static_dir: temp_dir.path().to_path_buf(),
            max_age: 3600,
            enable_csp: false,
            csp_nonce: None,
        };

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(config.clone()))
                .service(create_static_service(config)),
        )
        .await;

        // First request to get ETag
        let req = test::TestRequest::get().uri("/app.js").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());

        let etag = resp.headers().get("etag").unwrap().to_str().unwrap();
        println!("ETag value: '{}'", etag);
        assert!(etag.starts_with("W/"));

        // Second request with If-None-Match should return 304
        let req = test::TestRequest::get()
            .uri("/app.js")
            .insert_header(("if-none-match", etag))
            .to_request();
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 304);
    }

    #[actix_web::test]
    async fn test_csp_headers() {
        let temp_dir = create_test_static_dir().await;
        let config = StaticConfig {
            static_dir: temp_dir.path().to_path_buf(),
            max_age: 3600,
            enable_csp: true,
            csp_nonce: Some("test-nonce-123".to_string()),
        };

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(config.clone()))
                .service(create_static_service(config)),
        )
        .await;

        let req = test::TestRequest::get().uri("/").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());

        let csp = resp
            .headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("nonce-test-nonce-123"));
        assert!(csp.contains("default-src 'self'"));
    }

    #[actix_web::test]
    async fn test_cache_control_headers() {
        let temp_dir = create_test_static_dir().await;
        let config = StaticConfig {
            static_dir: temp_dir.path().to_path_buf(),
            max_age: 7200, // 2 hours
            enable_csp: false,
            csp_nonce: None,
        };

        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(config.clone()))
                .service(create_static_service(config)),
        )
        .await;

        let req = test::TestRequest::get().uri("/app.js").to_request();
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());

        let cache_control = resp
            .headers()
            .get("cache-control")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(cache_control.contains("public"));
        assert!(cache_control.contains("max-age=7200"));
    }
}
