//! ABOUTME: Actix-web route configuration and app factory creation
//! ABOUTME: Centralizes all route definitions and middleware setup

use crate::{
    middleware, models,
    routes::{admin, ai, alerts, auth as auth_routes, public, static_files, stream, streams},
    AppState,
};
use actix_web::{web, App, HttpRequest, HttpResponse};
use serde_json::json;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        auth_routes::login,
        public::me,
        stream::snapshot,
        stream::recent_snapshots,
        stream::mjpeg_stream,
        stream::start_stream,
        stream::stop_stream,
    ),
    components(
        schemas(
            models::LoginRequest,
            models::LoginResponse,
            models::UserInfo,
            models::AdminStreamInfo,
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
pub fn create_app(
    state: AppState,
) -> App<
    impl actix_web::dev::ServiceFactory<
        actix_web::dev::ServiceRequest,
        Config = (),
        Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
        Error = actix_web::Error,
        InitError = (),
    >,
> {
    let static_config = state.static_config.clone();
    let rate_limit_config = state.rate_limit_config.clone();

    // Create body limits config with per-endpoint overrides
    let body_limits_config = state.body_limits_config.clone().with_override(
        "/api/upload",
        state.body_limits_config.default_json_limit * 100,
    ); // Allow large uploads

    App::new()
        .app_data(web::Data::new(state))
        .app_data(web::Data::new(static_config.clone()))
        .wrap(actix_web::middleware::Logger::default())
        // Normalize paths: prefer no trailing slash
        .wrap(actix_web::middleware::NormalizePath::new(
            actix_web::middleware::TrailingSlash::Trim,
        ))
        .wrap(static_files::security_headers())
        // Apply body size limits globally
        .wrap(middleware::bodylimits::BodyLimits::new(body_limits_config))
        // Explicit absolute resources for settings to satisfy tests
        .service(
            web::resource("/api/settings/streams")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::list_streams_handler))
                .route(web::post().to(admin::create_stream_handler)),
        )
        .service(
            web::resource("/api/settings/streams/{id}")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::get_stream_handler))
                .route(web::put().to(admin::update_stream_handler))
                .route(web::delete().to(admin::delete_stream_handler)),
        )
        .service(
            web::resource("/api/settings/users")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::list_users_handler))
                .route(web::post().to(admin::create_user_handler)),
        )
        .service(
            web::resource("/api/settings/users/{id}")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::get_user_handler))
                .route(web::delete().to(admin::delete_user_handler)),
        )
        .service(
            web::resource("/api/settings/api-keys")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::list_api_keys_handler))
                .route(web::post().to(admin::create_api_key_handler)),
        )
        .service(
            web::resource("/api/settings/api-keys/{id}")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::delete().to(admin::delete_api_key_handler)),
        )
        .service(
            web::resource("/api/settings/updates/check")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::check_updates_handler)),
        )
        .service(
            web::resource("/api/settings/updates/apply")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::post().to(admin::apply_update_handler)),
        )
        .service(
            web::resource("/api/settings/updates/status")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::get_update_status_handler)),
        )
        .service(
            web::resource("/api/settings/_health")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(|| async { HttpResponse::Ok().json(json!({"ok": true})) }))
                .route(web::post().to(
                    |payload: actix_web::web::Json<serde_json::Value>| async move {
                        HttpResponse::Ok().json(payload.into_inner())
                    },
                )),
        )
        // Stream Import/Export endpoints (explicit absolute paths)
        .service(
            web::resource("/api/settings/streams/export")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::get().to(admin::export_streams)),
        )
        .service(
            web::resource("/api/settings/streams/import")
                .wrap(middleware::ratelimit::RateLimit::new(
                    rate_limit_config.clone(),
                ))
                .wrap(middleware::auth::RequireAuth::new())
                .route(web::post().to(admin::import_streams)),
        )
        .service(SwaggerUi::new("/docs/{_:.*}").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .service(
            web::scope("/api")
                // CRUD for streams (alias to existing template handlers)
                .service(
                    web::scope("/streams")
                        .wrap(middleware::ratelimit::RateLimit::new(
                            rate_limit_config.clone(),
                        ))
                        .wrap(middleware::auth::RequireAuth::new())
                        .route("", web::get().to(streams::list_streams))
                        .route("", web::post().to(streams::create_stream))
                        .route("/{id}", web::get().to(streams::get_stream))
                        .route("/{id}", web::put().to(streams::update_stream))
                        .route("/{id}", web::delete().to(streams::delete_stream)),
                )
                // Templates concept removed - only streams exist now
                .service(
                    web::scope("/auth")
                        // Apply rate limiting to auth endpoints (no auth required)
                        .wrap(middleware::ratelimit::RateLimit::new(
                            rate_limit_config.clone(),
                        ))
                        .service(auth_routes::login)
                        .service(auth_routes::setup_needed)
                        .service(auth_routes::setup_signup),
                )
                .service(
                    web::scope("/stream")
                        .wrap(middleware::ratelimit::RateLimit::new(
                            rate_limit_config.clone(),
                        ))
                        .wrap(middleware::auth::RequireAuth::new())
                        .service(stream::snapshot)
                        .service(stream::recent_snapshots)
                        .service(stream::mjpeg_stream)
                        .service(stream::start_stream)
                        .service(stream::stop_stream)
                        .service(stream::thumbnail)
                        .service(stream::stream_details)
                        .service(stream::live_stream),
                )
                .service(
                    web::scope("")
                        .wrap(middleware::ratelimit::RateLimit::new(
                            rate_limit_config.clone(),
                        ))
                        .wrap(middleware::auth::RequireAuth::new())
                        .service(public::me)
                        .service(public::alerts)
                        .service(public::health),
                )
                // Settings resources (absolute paths)
                .service(
                    web::resource("/settings/streams")
                        .wrap(middleware::ratelimit::RateLimit::new(
                            rate_limit_config.clone(),
                        ))
                        .wrap(middleware::auth::RequireAuth::new())
                        .route(web::get().to(admin::list_streams_handler))
                        .route(web::post().to(admin::create_stream_handler)),
                )
                .service(
                    web::scope("/settings")
                        .wrap(middleware::ratelimit::RateLimit::new(
                            rate_limit_config.clone(),
                        ))
                        .wrap(middleware::auth::RequireAuth::new())
                        // Streams
                        .service(
                            web::resource("streams")
                                .route(web::get().to(admin::list_streams_handler))
                                .route(web::post().to(admin::create_stream_handler)),
                        )
                        .service(
                            web::resource("streams/{id}")
                                .route(web::get().to(admin::get_stream_handler))
                                .route(web::put().to(admin::update_stream_handler))
                                .route(web::delete().to(admin::delete_stream_handler)),
                        )
                        // Export/Import
                        .service(
                            web::resource("streams/export")
                                .route(web::get().to(admin::export_streams)),
                        )
                        .service(
                            web::resource("streams/import")
                                .route(web::post().to(admin::import_streams)),
                        )
                        // Users
                        .service(
                            web::resource("users")
                                .route(web::get().to(admin::list_users_handler))
                                .route(web::post().to(admin::create_user_handler)),
                        )
                        .service(
                            web::resource("users/{id}")
                                .route(web::get().to(admin::get_user_handler))
                                .route(web::delete().to(admin::delete_user_handler)),
                        )
                        // API keys
                        .service(
                            web::resource("api-keys")
                                .route(web::get().to(admin::list_api_keys_handler))
                                .route(web::post().to(admin::create_api_key_handler)),
                        )
                        .service(
                            web::resource("api-keys/{id}")
                                .route(web::delete().to(admin::delete_api_key_handler)),
                        )
                        // Software Updates
                        .service(
                            web::resource("updates/check")
                                .route(web::get().to(admin::check_updates_handler)),
                        )
                        .service(
                            web::resource("updates/apply")
                                .route(web::post().to(admin::apply_update_handler)),
                        )
                        .service(
                            web::resource("updates/status")
                                .route(web::get().to(admin::get_update_status_handler)),
                        )
                        // Health
                        .service(
                            web::resource("_health")
                                .route(
                                    web::get().to(|| async {
                                        HttpResponse::Ok().json(json!({"ok": true}))
                                    }),
                                )
                                .route(web::post().to(
                                    |payload: actix_web::web::Json<serde_json::Value>| async move {
                                        HttpResponse::Ok().json(payload.into_inner())
                                    },
                                )),
                        ),
                )
                // Helpful 404 for unmatched API paths
                .default_service(web::to(|req: HttpRequest| async move {
                    let p = req.path().to_string();
                    info!(path = %p, "Unmatched API route");
                    HttpResponse::NotFound().json(json!({
                        "error": "Not Found",
                        "path": p
                    }))
                }))
                .configure(alerts::configure_alert_routes)
                .configure(ai::configure_ai_routes)
                .service(
                    web::scope("/debug").route(
                        "/test",
                        web::get()
                            .to(|| async { HttpResponse::Ok().json(json!({"debug": "working"})) }),
                    ),
                ),
        )
        // Static files service for assets directory
        .service(static_files::create_static_service(static_config))
    // TODO: Re-enable SPA fallback after fixing admin routes
    // .default_service(web::route().to(static_files::spa_fallback))
}
