//! ABOUTME: In-memory caching layer for frequently accessed database entities
//! ABOUTME: Provides LRU cache with TTL support for users, streams, and API keys

use crate::repositories::{api_keys::ApiKey, streams::Stream, users::User};
use linked_hash_map::LinkedHashMap;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

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
    access_order: LinkedHashMap<String, ()>,
    max_size: usize,
    ttl: Duration,
}

impl<T: Clone> LruCache<T> {
    fn new(max_size: usize, ttl: Duration) -> Self {
        Self {
            data: HashMap::new(),
            access_order: LinkedHashMap::new(),
            max_size,
            ttl,
        }
    }

    fn get(&mut self, key: &str) -> Option<T> {
        // Check if entry exists and is not expired
        if let Some(entry) = self.data.get(key) {
            if !entry.is_expired() {
                // Move to back (most recently used)
                self.access_order.remove(key);
                self.access_order.insert(key.to_string(), ());
                debug!("Cache hit for key: {}", key);
                return Some(entry.value.clone());
            } else {
                // Remove expired entry
                self.data.remove(key);
                self.access_order.remove(key);
                debug!("Cache miss (expired) for key: {}", key);
            }
        } else {
            debug!("Cache miss for key: {}", key);
        }
        None
    }

    fn put(&mut self, key: String, value: T) {
        // Remove existing entry if present
        if self.data.remove(&key).is_some() {
            self.access_order.remove(&key);
        }

        // Evict least recently used if at capacity
        while self.data.len() >= self.max_size {
            if let Some((lru_key, _)) = self.access_order.pop_front() {
                self.data.remove(&lru_key);
                debug!("Evicted LRU key: {}", lru_key);
            } else {
                break;
            }
        }

        // Insert new entry
        let entry = CacheEntry::new(value, self.ttl);
        self.data.insert(key.clone(), entry);
        self.access_order.insert(key.clone(), ());
        debug!("Cached key: {}", key);
    }

    fn invalidate(&mut self, key: &str) {
        if self.data.remove(key).is_some() {
            self.access_order.remove(key);
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
    users: Arc<RwLock<LruCache<User>>>,
    streams: Arc<RwLock<LruCache<Stream>>>,
    api_keys: Arc<RwLock<LruCache<ApiKey>>>,
    // Cache for user lookups by email (login optimization)
    users_by_email: Arc<RwLock<LruCache<User>>>,
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
    pub fn get_user(&self, id: &str) -> Option<User> {
        match self.users.write() {
            Ok(mut cache) => cache.get(id),
            Err(e) => {
                warn!("Failed to acquire user cache lock: {}", e);
                None
            }
        }
    }

    /// Cache user by ID
    pub fn cache_user(&self, user: User) {
        let id = user.id.clone();
        let email = user.email.clone();

        // Cache by ID
        if let Ok(mut cache) = self.users.write() {
            cache.put(id, user.clone());
        }

        // Also cache by email for login optimization
        if let Ok(mut cache) = self.users_by_email.write() {
            cache.put(email, user);
        }
    }

    /// Get user by email from cache
    pub fn get_user_by_email(&self, email: &str) -> Option<User> {
        match self.users_by_email.write() {
            Ok(mut cache) => cache.get(email),
            Err(e) => {
                warn!("Failed to acquire user email cache lock: {}", e);
                None
            }
        }
    }

    /// Invalidate user cache entries
    pub fn invalidate_user(&self, id: &str, email: Option<&str>) {
        if let Ok(mut cache) = self.users.write() {
            cache.invalidate(id);
        }
        if let Some(email) = email {
            if let Ok(mut cache) = self.users_by_email.write() {
                cache.invalidate(email);
            }
        }
    }

    /// Get stream by ID from cache
    pub fn get_stream(&self, id: &str) -> Option<Stream> {
        match self.streams.write() {
            Ok(mut cache) => cache.get(id),
            Err(e) => {
                warn!("Failed to acquire stream cache lock: {}", e);
                None
            }
        }
    }

    /// Cache stream by ID
    pub fn cache_stream(&self, stream: Stream) {
        let id = stream.id.clone();
        match self.streams.write() {
            Ok(mut cache) => cache.put(id, stream),
            Err(e) => warn!("Failed to acquire stream cache lock for caching: {}", e),
        }
    }

    /// Invalidate stream cache entry
    pub fn invalidate_stream(&self, id: &str) {
        match self.streams.write() {
            Ok(mut cache) => cache.invalidate(id),
            Err(e) => warn!(
                "Failed to acquire stream cache lock for invalidation: {}",
                e
            ),
        }
    }

    /// Clear all stream cache entries
    pub fn clear_streams(&self) {
        match self.streams.write() {
            Ok(mut cache) => cache.clear(),
            Err(e) => warn!("Failed to acquire stream cache lock for clearing: {}", e),
        }
    }

    /// Get API key by hash from cache
    pub fn get_api_key(&self, hash: &str) -> Option<ApiKey> {
        match self.api_keys.write() {
            Ok(mut cache) => cache.get(hash),
            Err(e) => {
                warn!("Failed to acquire API key cache lock: {}", e);
                None
            }
        }
    }

    /// Cache API key by hash
    pub fn cache_api_key(&self, api_key: ApiKey) {
        let hash = api_key.key_hash.clone();
        if let Ok(mut cache) = self.api_keys.write() {
            cache.put(hash, api_key);
        }
    }

    /// Invalidate API key cache entry
    pub fn invalidate_api_key(&self, hash: &str) {
        if let Ok(mut cache) = self.api_keys.write() {
            cache.invalidate(hash);
        }
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let users_size = self.users.read().map(|c| c.size()).unwrap_or(0);
        let streams_size = self.streams.read().map(|c| c.size()).unwrap_or(0);
        let api_keys_size = self.api_keys.read().map(|c| c.size()).unwrap_or(0);
        let users_by_email_size = self.users_by_email.read().map(|c| c.size()).unwrap_or(0);

        CacheStats {
            users_count: users_size,
            streams_count: streams_size,
            api_keys_count: api_keys_size,
            users_by_email_count: users_by_email_size,
        }
    }

    /// Clear all caches
    pub fn clear_all(&self) {
        if let Ok(mut cache) = self.users.write() {
            cache.clear();
        }
        if let Ok(mut cache) = self.streams.write() {
            cache.clear();
        }
        if let Ok(mut cache) = self.api_keys.write() {
            cache.clear();
        }
        if let Ok(mut cache) = self.users_by_email.write() {
            cache.clear();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lru_eviction_order() {
        let mut cache = LruCache::new(3, Duration::from_secs(60));
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);
        cache.put("c".to_string(), 3);

        // Access 'a' to refresh its position
        assert_eq!(cache.get("a"), Some(1));

        // Adding a fourth entry should evict 'b'
        cache.put("d".to_string(), 4);

        assert!(cache.get("b").is_none());
        assert_eq!(cache.get("a"), Some(1));
        assert_eq!(cache.get("c"), Some(3));
        assert_eq!(cache.get("d"), Some(4));
    }

    #[test]
    fn updating_existing_key_does_not_evict_others() {
        let mut cache = LruCache::new(2, Duration::from_secs(60));
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);

        // Update existing 'a'
        cache.put("a".to_string(), 3);

        // Cache still has two entries
        assert_eq!(cache.size(), 2);

        // Adding new entry should evict 'b' because 'a' was refreshed
        cache.put("c".to_string(), 4);
        assert!(cache.get("b").is_none());
        assert_eq!(cache.get("a"), Some(3));
        assert_eq!(cache.get("c"), Some(4));
    }
}
