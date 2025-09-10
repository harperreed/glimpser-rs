//! ABOUTME: In-memory caching layer for frequently accessed database entities
//! ABOUTME: Provides LRU cache with TTL support for users, streams, and API keys

use crate::repositories::{api_keys::ApiKey, streams::Stream, users::User};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::debug;

/// Cache entry with TTL support
#[derive(Debug, Clone)]
struct CacheEntry<T> {
    value: T,
    expires_at: Instant,
}

impl<T> CacheEntry<T> {
    fn new(value: T, ttl: Duration) -> Self {
        Self {
            value,
            expires_at: Instant::now() + ttl,
        }
    }

    fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

/// Generic LRU cache with TTL support
#[derive(Debug)]
struct LruCache<T: Clone> {
    data: HashMap<String, CacheEntry<T>>,
    access_order: Vec<String>,
    max_size: usize,
    ttl: Duration,
}

impl<T: Clone> LruCache<T> {
    fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            data: HashMap::new(),
            access_order: Vec::new(),
            max_size,
            ttl,
        }
    }

    fn get(&mut self, key: &str) -> Option<T> {
        // Check if entry exists and is not expired
        if let Some(entry) = self.data.get(key) {
            if !entry.is_expired() {
                // Move to front (most recently used)
                if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                    let key_owned = self.access_order.remove(pos);
                    self.access_order.push(key_owned);
                }
                debug!("Cache hit for key: {}", key);
                return Some(entry.value.clone());
            } else {
                // Remove expired entry
                self.data.remove(key);
                if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                    self.access_order.remove(pos);
                }
                debug!("Cache miss (expired) for key: {}", key);
            }
        } else {
            debug!("Cache miss for key: {}", key);
        }
        None
    }

    fn put(&mut self, key: String, value: T) {
        // Remove existing entry if present
        if self.data.contains_key(&key) {
            if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                self.access_order.remove(pos);
            }
        }

        // Evict least recently used if at capacity
        while self.data.len() >= self.max_size {
            if let Some(lru_key) = self.access_order.first().cloned() {
                self.data.remove(&lru_key);
                self.access_order.remove(0);
                debug!("Evicted LRU key: {}", lru_key);
            } else {
                break;
            }
        }

        // Insert new entry
        let entry = CacheEntry::new(value, self.ttl);
        self.data.insert(key.clone(), entry);
        self.access_order.push(key.clone());
        debug!("Cached key: {}", key);
    }

    fn invalidate(&mut self, key: &str) {
        if self.data.remove(key).is_some() {
            if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                self.access_order.remove(pos);
            }
            debug!("Invalidated cache key: {}", key);
        }
    }

    fn clear(&mut self) {
        self.data.clear();
        self.access_order.clear();
        debug!("Cleared cache");
    }

    fn size(&self) -> usize {
        self.data.len()
    }
}

/// Application-level cache manager for database entities
#[derive(Debug)]
pub struct DatabaseCache {
    users: Arc<RwLock<LruCache<Arc<User>>>>,
    streams: Arc<RwLock<LruCache<Arc<Stream>>>>,
    api_keys: Arc<RwLock<LruCache<Arc<ApiKey>>>>,
    // Cache for user lookups by email (login optimization)
    users_by_email: Arc<RwLock<LruCache<Arc<User>>>>,
}

impl DatabaseCache {
    /// Create a new database cache with default settings
    pub fn new() -> Self {
        Self {
            users: Arc::new(RwLock::new(LruCache::new(100, Duration::from_secs(300)))), // 5 min TTL
            streams: Arc::new(RwLock::new(LruCache::new(200, Duration::from_secs(180)))), // 3 min TTL
            api_keys: Arc::new(RwLock::new(LruCache::new(50, Duration::from_secs(600)))), // 10 min TTL
            users_by_email: Arc::new(RwLock::new(LruCache::new(100, Duration::from_secs(300)))),
        }
    }

    /// Get user by ID from cache
    pub async fn get_user(&self, id: &str) -> Option<Arc<User>> {
        let mut cache = self.users.write().await;
        cache.get(id)
    }

    /// Cache user by ID
    pub async fn cache_user(&self, user: Arc<User>) {
        let id = user.id.clone();
        let email = user.email.clone();

        // Cache by ID
        {
            let mut cache = self.users.write().await;
            cache.put(id, user.clone());
        }

        // Also cache by email for login optimization
        {
            let mut cache = self.users_by_email.write().await;
            cache.put(email, user);
        }
    }

    /// Get user by email from cache
    pub async fn get_user_by_email(&self, email: &str) -> Option<Arc<User>> {
        let mut cache = self.users_by_email.write().await;
        cache.get(email)
    }

    /// Invalidate user cache entries
    pub async fn invalidate_user(&self, id: &str, email: Option<&str>) {
        {
            let mut cache = self.users.write().await;
            cache.invalidate(id);
        }
        if let Some(email) = email {
            let mut cache = self.users_by_email.write().await;
            cache.invalidate(email);
        }
    }

    /// Get stream by ID from cache
    pub async fn get_stream(&self, id: &str) -> Option<Arc<Stream>> {
        let mut cache = self.streams.write().await;
        cache.get(id)
    }

    /// Cache stream by ID
    pub async fn cache_stream(&self, stream: Arc<Stream>) {
        let id = stream.id.clone();
        let mut cache = self.streams.write().await;
        cache.put(id, stream);
    }

    /// Invalidate stream cache entry
    pub async fn invalidate_stream(&self, id: &str) {
        let mut cache = self.streams.write().await;
        cache.invalidate(id);
    }

    /// Get API key by hash from cache
    pub async fn get_api_key(&self, hash: &str) -> Option<Arc<ApiKey>> {
        let mut cache = self.api_keys.write().await;
        cache.get(hash)
    }

    /// Cache API key by hash
    pub async fn cache_api_key(&self, api_key: Arc<ApiKey>) {
        let hash = api_key.key_hash.clone();
        let mut cache = self.api_keys.write().await;
        cache.put(hash, api_key);
    }

    /// Invalidate API key cache entry
    pub async fn invalidate_api_key(&self, hash: &str) {
        let mut cache = self.api_keys.write().await;
        cache.invalidate(hash);
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let users_size = self.users.read().await.size();
        let streams_size = self.streams.read().await.size();
        let api_keys_size = self.api_keys.read().await.size();
        let users_by_email_size = self.users_by_email.read().await.size();

        CacheStats {
            users_count: users_size,
            streams_count: streams_size,
            api_keys_count: api_keys_size,
            users_by_email_count: users_by_email_size,
        }
    }

    /// Clear all caches
    pub async fn clear_all(&self) {
        self.users.write().await.clear();
        self.streams.write().await.clear();
        self.api_keys.write().await.clear();
        self.users_by_email.write().await.clear();
    }
}

impl Default for DatabaseCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics for monitoring
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub users_count: usize,
    pub streams_count: usize,
    pub api_keys_count: usize,
    pub users_by_email_count: usize,
}

impl CacheStats {
    pub fn total_entries(&self) -> usize {
        self.users_count + self.streams_count + self.api_keys_count + self.users_by_email_count
    }
}
