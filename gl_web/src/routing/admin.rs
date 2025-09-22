//! ABOUTME: Admin route configuration for settings management
//! ABOUTME: Consolidates duplicate admin endpoints with proper middleware

use crate::routes::admin;
use actix_web::{web, HttpResponse};
use serde_json::json;

/// Configure admin routes - consolidates all duplicate admin endpoints
/// Note: Rate limiting and auth middleware must be applied at the parent scope
pub fn configure_admin_routes(cfg: &mut web::ServiceConfig) {
    cfg
        // Stream management
        .service(
            web::resource("/streams")
                .route(web::get().to(admin::list_streams_handler))
                .route(web::post().to(admin::create_stream_handler)),
        )
        .service(
            web::resource("/streams/{id}")
                .route(web::get().to(admin::get_stream_handler))
                .route(web::put().to(admin::update_stream_handler))
                .route(web::delete().to(admin::delete_stream_handler)),
        )
        // Stream import/export
        .service(web::resource("/streams/export").route(web::get().to(admin::export_streams)))
        .service(web::resource("/streams/import").route(web::post().to(admin::import_streams)))
        // User management
        .service(
            web::resource("/users")
                .route(web::get().to(admin::list_users_handler))
                .route(web::post().to(admin::create_user_handler)),
        )
        .service(
            web::resource("/users/{id}")
                .route(web::get().to(admin::get_user_handler))
                .route(web::delete().to(admin::delete_user_handler)),
        )
        // API key management
        .service(
            web::resource("/api-keys")
                .route(web::get().to(admin::list_api_keys_handler))
                .route(web::post().to(admin::create_api_key_handler)),
        )
        .service(
            web::resource("/api-keys/{id}").route(web::delete().to(admin::delete_api_key_handler)),
        )
        // Software updates
        .service(web::resource("/updates/check").route(web::get().to(admin::check_updates_handler)))
        .service(web::resource("/updates/apply").route(web::post().to(admin::apply_update_handler)))
        .service(
            web::resource("/updates/status").route(web::get().to(admin::get_update_status_handler)),
        )
        // Health endpoint
        .service(
            web::resource("/_health")
                .route(web::get().to(|| async { HttpResponse::Ok().json(json!({"ok": true})) }))
                .route(
                    web::post().to(|payload: web::Json<serde_json::Value>| async move {
                        HttpResponse::Ok().json(payload.into_inner())
                    }),
                ),
        );
}
