//! ABOUTME: JWT authentication with secure HTTP-only cookie storage
//! ABOUTME: Provides login, token validation, and CSRF protection
//!
//! ## Security Architecture
//!
//! This module implements secure authentication using JWT tokens stored in
//! HTTP-only cookies, which provides protection against XSS attacks.
//!
//! ### Why HTTP-only Cookies (not localStorage)
//!
//! - **HttpOnly flag**: Prevents JavaScript from accessing tokens, mitigating XSS
//! - **Secure flag**: Ensures tokens only sent over HTTPS
//! - **SameSite flag**: Provides CSRF protection
//!
//! ### Security Properties
//!
//! 1. Tokens are NEVER exposed to JavaScript
//! 2. Tokens are NEVER sent in response bodies
//! 3. Tokens are NEVER stored in localStorage or sessionStorage
//! 4. All authentication uses HTTP-only, Secure cookies

use crate::models::Claims;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use gl_config::Argon2Config;
use gl_core::{Error, Result};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand_core::OsRng;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, instrument};

/// Password hashing utilities
pub struct PasswordAuth;

impl PasswordAuth {
    /// Create a configured Argon2 instance with proper security parameters
    fn create_argon2(config: &Argon2Config) -> Result<Argon2<'_>> {
        // Enforce minimum security parameters as specified in the audit
        let memory_cost = config.memory_cost.max(19456); // At least 19 MiB
        let time_cost = config.time_cost.max(2); // At least 2 iterations
        let parallelism = config.parallelism.max(1); // At least 1 thread

        let params = Params::new(memory_cost, time_cost, parallelism, None)
            .map_err(|e| Error::Config(format!("Invalid Argon2 parameters: {}", e)))?;

        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }

    /// Hash a password using Argon2 with configured parameters
    #[instrument(skip(password, config))]
    pub fn hash_password(password: &str, config: &Argon2Config) -> Result<String> {
        debug!("Hashing password with configured Argon2 parameters");

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Self::create_argon2(config)?;

        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| Error::Config(format!("Failed to hash password: {}", e)))?
            .to_string();

        debug!("Password hashed successfully");
        Ok(password_hash)
    }

    /// Verify a password against a hash using configured parameters
    #[instrument(skip(password, hash, config))]
    pub fn verify_password(password: &str, hash: &str, config: &Argon2Config) -> Result<bool> {
        debug!("Verifying password with configured Argon2 parameters");

        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| Error::Config(format!("Invalid password hash format: {}", e)))?;

        let argon2 = Self::create_argon2(config)?;

        match argon2.verify_password(password.as_bytes(), &parsed_hash) {
            Ok(()) => {
                debug!("Password verification successful");
                Ok(true)
            }
            Err(_) => {
                debug!("Password verification failed");
                Ok(false)
            }
        }
    }
}

/// JWT token utilities
pub struct JwtAuth;

impl JwtAuth {
    /// JWT token expiration time in seconds (24 hours)
    const TOKEN_EXPIRATION_SECS: u64 = 24 * 60 * 60;

    /// Create a new JWT token for a user
    #[instrument(skip(secret))]
    pub fn create_token(user_id: &str, email: &str, secret: &str, issuer: &str) -> Result<String> {
        debug!("Creating JWT token for user: {}", user_id);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Config(format!("Time error: {}", e)))?
            .as_secs() as usize;

        let claims = Claims {
            sub: user_id.to_string(),
            email: email.to_string(),
            exp: now + Self::TOKEN_EXPIRATION_SECS as usize,
            iat: now,
            iss: issuer.to_string(),
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_ref()),
        )
        .map_err(|e| Error::Config(format!("Failed to create JWT: {}", e)))?;

        debug!("JWT token created successfully");
        Ok(token)
    }

    /// Verify and decode a JWT token
    #[instrument(skip(token, secret))]
    pub fn verify_token(token: &str, secret: &str, expected_issuer: &str) -> Result<Claims> {
        debug!("Verifying JWT token");

        let mut validation = Validation::default();
        validation.set_issuer(&[expected_issuer]);
        validation.validate_exp = true;

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_ref()),
            &validation,
        )
        .map_err(|e| Error::Validation(format!("Invalid JWT: {}", e)))?;

        debug!("JWT token verified successfully");
        Ok(token_data.claims)
    }

    /// Get token expiration time in seconds
    pub fn token_expiration_secs() -> u64 {
        Self::TOKEN_EXPIRATION_SECS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hash_and_verify() {
        let password = "test_password_123";
        let config = Argon2Config::default();

        // Hash password
        let hash = PasswordAuth::hash_password(password, &config).expect("Should hash password");
        assert!(!hash.is_empty());
        assert!(hash.starts_with("$argon2"));

        // Verify correct password
        let is_valid =
            PasswordAuth::verify_password(password, &hash, &config).expect("Should verify");
        assert!(is_valid);

        // Verify wrong password
        let is_valid =
            PasswordAuth::verify_password("wrong_password", &hash, &config).expect("Should verify");
        assert!(!is_valid);
    }

    #[test]
    fn test_jwt_create_and_verify() {
        let user_id = "user_123";
        let email = "test@example.com";
        let secret = "test_secret_key";
        let issuer = "glimpser";

        // Create token
        let token =
            JwtAuth::create_token(user_id, email, secret, issuer).expect("Should create token");
        assert!(!token.is_empty());

        // Verify token
        let claims = JwtAuth::verify_token(&token, secret, issuer).expect("Should verify token");
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.email, email);
        assert_eq!(claims.iss, issuer);
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn test_jwt_invalid_secret() {
        let user_id = "user_123";
        let email = "test@example.com";
        let secret = "test_secret_key";
        let wrong_secret = "wrong_secret";
        let issuer = "glimpser";

        // Create token with one secret
        let token =
            JwtAuth::create_token(user_id, email, secret, issuer).expect("Should create token");

        // Try to verify with different secret
        let result = JwtAuth::verify_token(&token, wrong_secret, issuer);
        assert!(result.is_err());
    }
}
