//! ABOUTME: Tests for DatabaseCache asynchronous behavior and Arc storage
//! ABOUTME: Validates caching and retrieval without extra cloning

use std::sync::Arc;
use gl_db::DatabaseCache;
use gl_db::repositories::users::User;

#[tokio::test]
async fn caches_and_retrieves_user_by_id() {
    let cache = DatabaseCache::new();
    let user = Arc::new(User {
        id: "user1".into(),
        username: "u1".into(),
        email: "u1@example.com".into(),
        password_hash: "hash".into(),
        is_active: Some(true),
        created_at: "now".into(),
        updated_at: "now".into(),
    });

    cache.cache_user(user.clone()).await;

    let fetched = cache.get_user("user1").await.unwrap();
    assert!(Arc::ptr_eq(&fetched, &user));
}
