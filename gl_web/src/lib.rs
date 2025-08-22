//! ABOUTME: Web API layer with authentication and routing
//! ABOUTME: Provides REST endpoints and OpenAPI documentation

use actix_web::{web, App, HttpServer};
use gl_core::Result;
use gl_db::Db;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

pub mod auth;
pub mod middleware;
pub mod models;
pub mod routes;

use routes::{admin, alerts, auth as auth_routes, public, stream, templates};

/// Application state shared across all handlers
#[derive(Debug, Clone)]
pub struct AppState {
    pub db: Db,
    pub jwt_secret: String,
}

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        auth_routes::login,
        public::me,
        stream::snapshot,
        stream::mjpeg_stream,
    ),
    components(
        schemas(
            models::LoginRequest,
            models::LoginResponse,
            models::UserInfo,
            models::TemplateInfo,
            models::ErrorResponse,
        ),
    ),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "public", description = "Public endpoints"),
        (name = "admin", description = "Admin endpoints"),
        (name = "stream", description = "Stream snapshot endpoints"),
    )
)]
pub struct ApiDoc;

/// Create the main web application service factory
pub fn create_app(state: AppState) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    App::new()
        .app_data(web::Data::new(state))
        .wrap(actix_web::middleware::Logger::default())
        .service(
            SwaggerUi::new("/docs/{_:.*}")
                .url("/api-docs/openapi.json", ApiDoc::openapi())
        )
        .service(
            web::scope("/api")
                .service(
                    web::scope("/auth")
                        .service(auth_routes::login)
                )
                .service(
                    web::scope("")
                        .wrap(middleware::auth::RequireAuth::new())
                        .service(public::me)
                )
                .service(
                    web::scope("/admin")
                        .wrap(middleware::auth::RequireAuth::new())
                        .wrap(middleware::rbac::RequireRole::admin())
                        .service(admin::list_templates)
                )
                .service(
                    web::scope("/stream")
                        .wrap(middleware::auth::RequireAuth::new())
                        .service(stream::snapshot)
                        .service(stream::mjpeg_stream)
                )
        )
        .configure(alerts::configure_alert_routes)
        .configure(templates::configure_template_routes)
}

/// Start the web server
pub async fn start_server(bind_addr: &str, state: AppState) -> Result<()> {
    tracing::info!("Starting web server on {}", bind_addr);
    
    HttpServer::new(move || create_app(state.clone()))
        .bind(bind_addr)
        .map_err(|e| gl_core::Error::Config(format!("Failed to bind web server: {}", e)))?
        .run()
        .await
        .map_err(|e| gl_core::Error::Config(format!("Web server error: {}", e)))?;
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LoginRequest, UserInfo};
    use actix_web::test;
    use gl_core::Id;
    use gl_db::{Db, UserRepository, CreateUserRequest};
    use crate::auth::PasswordAuth;
    
    async fn create_test_app_state() -> AppState {
        let test_id = Id::new().to_string();
        let db_path = format!("test_web_{}.db", test_id);
        let db = Db::new(&db_path).await.expect("Failed to create test database");
        
        AppState {
            db,
            jwt_secret: "test_secret_key_32_characters_minimum".to_string(),
        }
    }
    
    async fn create_test_user(state: &AppState, email: &str, password: &str, role: &str) -> gl_db::User {
        let user_repo = UserRepository::new(state.db.pool());
        let password_hash = PasswordAuth::hash_password(password).expect("Failed to hash password");
        
        let create_request = CreateUserRequest {
            username: email.split('@').next().unwrap().to_string(),
            email: email.to_string(),
            password_hash,
            role: role.to_string(),
        };
        
        user_repo.create(create_request).await.expect("Failed to create test user")
    }
    
    #[actix_web::test]
    async fn test_login_success() {
        let state = create_test_app_state().await;
        let _user = create_test_user(&state, "test@example.com", "password123", "admin").await;
        
        let app = test::init_service(create_app(state)).await;
        
        let login_request = LoginRequest {
            email: "test@example.com".to_string(),
            password: "password123".to_string(),
        };
        
        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(&login_request)
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert!(body["access_token"].is_string());
        assert_eq!(body["token_type"], "Bearer");
        assert!(body["user"]["email"].as_str().unwrap() == "test@example.com");
    }
    
    #[actix_web::test]
    async fn test_login_invalid_credentials() {
        let state = create_test_app_state().await;
        let _user = create_test_user(&state, "test@example.com", "password123", "admin").await;
        
        let app = test::init_service(create_app(state)).await;
        
        let login_request = LoginRequest {
            email: "test@example.com".to_string(),
            password: "wrong_password".to_string(),
        };
        
        let req = test::TestRequest::post()
            .uri("/api/auth/login")
            .set_json(&login_request)
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }
    
    #[actix_web::test]
    async fn test_me_endpoint_authenticated() {
        let state = create_test_app_state().await;
        let user = create_test_user(&state, "test@example.com", "password123", "admin").await;
        
        // Create JWT token
        let token = crate::auth::JwtAuth::create_token(
            &user.id,
            &user.email,
            &user.role,
            &state.jwt_secret,
        ).expect("Failed to create token");
        
        let app = test::init_service(create_app(state)).await;
        
        let req = test::TestRequest::get()
            .uri("/api/me")
            .insert_header(("authorization", format!("Bearer {}", token)))
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        
        let body: UserInfo = test::read_body_json(resp).await;
        assert_eq!(body.email, "test@example.com");
        assert_eq!(body.role, "admin");
    }
    
    #[actix_web::test]
    async fn test_me_endpoint_unauthenticated() {
        let state = create_test_app_state().await;
        let app = test::init_service(create_app(state)).await;
        
        let req = test::TestRequest::get()
            .uri("/api/me")
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 401);
    }
    
    #[actix_web::test]
    async fn test_admin_endpoint_requires_admin() {
        let state = create_test_app_state().await;
        let user = create_test_user(&state, "viewer@example.com", "password123", "viewer").await;
        
        // Create JWT token for viewer
        let token = crate::auth::JwtAuth::create_token(
            &user.id,
            &user.email,
            &user.role,
            &state.jwt_secret,
        ).expect("Failed to create token");
        
        let app = test::init_service(create_app(state)).await;
        
        let req = test::TestRequest::get()
            .uri("/api/admin/templates")
            .insert_header(("authorization", format!("Bearer {}", token)))
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert_eq!(resp.status(), 403); // Forbidden
    }
    
    #[actix_web::test]
    async fn test_admin_endpoint_allows_admin() {
        let state = create_test_app_state().await;
        let user = create_test_user(&state, "admin@example.com", "password123", "admin").await;
        
        // Create JWT token for admin
        let token = crate::auth::JwtAuth::create_token(
            &user.id,
            &user.email,
            &user.role,
            &state.jwt_secret,
        ).expect("Failed to create token");
        
        let app = test::init_service(create_app(state)).await;
        
        let req = test::TestRequest::get()
            .uri("/api/admin/templates")
            .insert_header(("authorization", format!("Bearer {}", token)))
            .to_request();
        
        let resp = test::call_service(&app, req).await;
        assert!(resp.status().is_success());
        
        let body: serde_json::Value = test::read_body_json(resp).await;
        assert!(body.is_array());
    }
}
