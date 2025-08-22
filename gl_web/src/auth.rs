//! ABOUTME: Authentication utilities for password hashing and JWT operations
//! ABOUTME: Provides secure password verification and JWT token management

use crate::models::Claims;
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use gl_core::{Error, Result};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand_core::OsRng;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, instrument};

/// Password hashing utilities
pub struct PasswordAuth;

impl PasswordAuth {
    /// Hash a password using Argon2
    #[instrument(skip(password))]
    pub fn hash_password(password: &str) -> Result<String> {
        debug!("Hashing password");

        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();

        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| Error::Config(format!("Failed to hash password: {}", e)))?
            .to_string();

        debug!("Password hashed successfully");
        Ok(password_hash)
    }

    /// Verify a password against a hash
    #[instrument(skip(password, hash))]
    pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
        debug!("Verifying password");

        let parsed_hash = PasswordHash::new(hash)
            .map_err(|e| Error::Config(format!("Invalid password hash format: {}", e)))?;

        let argon2 = Argon2::default();

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
    pub fn create_token(user_id: &str, email: &str, role: &str, secret: &str) -> Result<String> {
        debug!("Creating JWT token for user: {}", user_id);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Config(format!("Time error: {}", e)))?
            .as_secs() as usize;

        let claims = Claims {
            sub: user_id.to_string(),
            email: email.to_string(),
            role: role.to_string(),
            exp: now + Self::TOKEN_EXPIRATION_SECS as usize,
            iat: now,
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
    pub fn verify_token(token: &str, secret: &str) -> Result<Claims> {
        debug!("Verifying JWT token");

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(secret.as_ref()),
            &Validation::default(),
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
    use crate::models::Role;
    use std::str::FromStr;

    #[test]
    fn test_password_hash_and_verify() {
        let password = "test_password_123";

        // Hash password
        let hash = PasswordAuth::hash_password(password).expect("Should hash password");
        assert!(!hash.is_empty());
        assert!(hash.starts_with("$argon2"));

        // Verify correct password
        let is_valid = PasswordAuth::verify_password(password, &hash).expect("Should verify");
        assert!(is_valid);

        // Verify wrong password
        let is_valid =
            PasswordAuth::verify_password("wrong_password", &hash).expect("Should verify");
        assert!(!is_valid);
    }

    #[test]
    fn test_jwt_create_and_verify() {
        let user_id = "user_123";
        let email = "test@example.com";
        let role = "admin";
        let secret = "test_secret_key";

        // Create token
        let token =
            JwtAuth::create_token(user_id, email, role, secret).expect("Should create token");
        assert!(!token.is_empty());

        // Verify token
        let claims = JwtAuth::verify_token(&token, secret).expect("Should verify token");
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.email, email);
        assert_eq!(claims.role, role);
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn test_jwt_invalid_secret() {
        let user_id = "user_123";
        let email = "test@example.com";
        let role = "admin";
        let secret = "test_secret_key";
        let wrong_secret = "wrong_secret";

        // Create token with one secret
        let token =
            JwtAuth::create_token(user_id, email, role, secret).expect("Should create token");

        // Try to verify with different secret
        let result = JwtAuth::verify_token(&token, wrong_secret);
        assert!(result.is_err());
    }

    #[test]
    fn test_role_enum() {
        assert_eq!(Role::from_str("admin"), Ok(Role::Admin));
        assert_eq!(Role::from_str("ADMIN"), Ok(Role::Admin));
        assert_eq!(Role::from_str("operator"), Ok(Role::Operator));
        assert_eq!(Role::from_str("viewer"), Ok(Role::Viewer));
        assert!(Role::from_str("invalid").is_err());

        assert_eq!(Role::Admin.as_str(), "admin");
        assert_eq!(Role::Operator.as_str(), "operator");
        assert_eq!(Role::Viewer.as_str(), "viewer");
    }
}
