//! ABOUTME: Ed25519 signature verification for update integrity
//! ABOUTME: Provides cryptographic verification of downloaded binaries

use bytes::Bytes;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use gl_core::Result;
use sha2::{Digest, Sha256};
use tracing::{debug, info};

/// Ed25519 signature verifier for update binaries
#[derive(Debug)]
pub struct SignatureVerifier {
    public_key: VerifyingKey,
}

impl SignatureVerifier {
    /// Create a new signature verifier with the given public key
    ///
    /// # Arguments
    /// * `public_key_hex` - Public key as hex-encoded string
    pub fn new(public_key_hex: &str) -> Result<Self> {
        if public_key_hex.is_empty() {
            return Err(gl_core::Error::Config(
                "Public key cannot be empty".to_string(),
            ));
        }

        let key_bytes = hex::decode(public_key_hex)
            .map_err(|e| gl_core::Error::Config(format!("Invalid hex public key: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(gl_core::Error::Config(format!(
                "Public key must be 32 bytes, got {}",
                key_bytes.len()
            )));
        }

        let public_key = VerifyingKey::from_bytes(
            &key_bytes
                .try_into()
                .map_err(|_| gl_core::Error::Config("Failed to convert key bytes".to_string()))?,
        )
        .map_err(|e| gl_core::Error::Config(format!("Invalid public key: {}", e)))?;

        info!("Signature verifier initialized with public key");

        Ok(Self { public_key })
    }

    /// Verify a signature against binary data
    ///
    /// # Arguments
    /// * `data` - The binary data to verify
    /// * `signature_hex` - The signature as hex-encoded string
    pub fn verify(&self, data: &Bytes, signature_hex: &str) -> Result<()> {
        debug!("Verifying signature for {} bytes of data", data.len());

        // Decode the signature
        let signature_bytes = hex::decode(signature_hex)
            .map_err(|e| gl_core::Error::Validation(format!("Invalid hex signature: {}", e)))?;

        if signature_bytes.len() != 64 {
            return Err(gl_core::Error::Validation(format!(
                "Signature must be 64 bytes, got {}",
                signature_bytes.len()
            )));
        }

        let signature = Signature::from_bytes(&signature_bytes.try_into().map_err(|_| {
            gl_core::Error::Validation("Failed to convert signature bytes".to_string())
        })?);

        // Hash the data
        let hash = Sha256::digest(data);

        debug!("Data SHA256: {}", hex::encode(hash));

        // Verify the signature
        self.public_key.verify(&hash, &signature).map_err(|e| {
            gl_core::Error::Validation(format!("Signature verification failed: {}", e))
        })?;

        info!("Signature verification successful");
        Ok(())
    }

    /// Get the public key as hex string
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.public_key.as_bytes())
    }

    /// Verify signature with custom hash function
    /// This allows verifying against pre-hashed data if needed
    pub fn verify_hash(&self, hash: &[u8], signature_hex: &str) -> Result<()> {
        debug!("Verifying signature against {} bytes of hash", hash.len());

        let signature_bytes = hex::decode(signature_hex)
            .map_err(|e| gl_core::Error::Validation(format!("Invalid hex signature: {}", e)))?;

        if signature_bytes.len() != 64 {
            return Err(gl_core::Error::Validation(format!(
                "Signature must be 64 bytes, got {}",
                signature_bytes.len()
            )));
        }

        let signature = Signature::from_bytes(&signature_bytes.try_into().map_err(|_| {
            gl_core::Error::Validation("Failed to convert signature bytes".to_string())
        })?);

        self.public_key.verify(hash, &signature).map_err(|e| {
            gl_core::Error::Validation(format!("Hash signature verification failed: {}", e))
        })?;

        info!("Hash signature verification successful");
        Ok(())
    }
}

/// Helper functions for signature generation (typically used in CI/release processes)
pub mod signing {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand_core::{OsRng, RngCore};

    /// Generate a new Ed25519 key pair
    /// Returns (private_key_hex, public_key_hex)
    pub fn generate_keypair() -> (String, String) {
        let mut seed = [0u8; 32];
        OsRng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();

        (
            hex::encode(signing_key.to_bytes()),
            hex::encode(verifying_key.as_bytes()),
        )
    }

    /// Sign data with a private key
    ///
    /// # Arguments
    /// * `data` - The data to sign
    /// * `private_key_hex` - Private key as hex-encoded string
    pub fn sign_data(data: &[u8], private_key_hex: &str) -> Result<String> {
        let key_bytes = hex::decode(private_key_hex)
            .map_err(|e| gl_core::Error::Config(format!("Invalid hex private key: {}", e)))?;

        if key_bytes.len() != 32 {
            return Err(gl_core::Error::Config(format!(
                "Private key must be 32 bytes, got {}",
                key_bytes.len()
            )));
        }

        let signing_key = SigningKey::from_bytes(
            &key_bytes
                .try_into()
                .map_err(|_| gl_core::Error::Config("Failed to convert key bytes".to_string()))?,
        );

        // Hash the data
        let hash = Sha256::digest(data);

        // Sign the hash
        let signature = signing_key.sign(&hash);

        Ok(hex::encode(signature.to_bytes()))
    }

    /// Sign a file with a private key
    pub fn sign_file(file_path: &std::path::Path, private_key_hex: &str) -> Result<String> {
        let data = std::fs::read(file_path)
            .map_err(|e| gl_core::Error::External(format!("Failed to read file: {}", e)))?;

        sign_data(&data, private_key_hex)
    }
}

#[cfg(test)]
mod tests {
    use super::signing::*;
    use super::*;

    #[test]
    fn test_generate_keypair() {
        let (private_key, public_key) = generate_keypair();

        assert_eq!(private_key.len(), 64); // 32 bytes * 2 (hex)
        assert_eq!(public_key.len(), 64); // 32 bytes * 2 (hex)

        // Should be valid hex
        assert!(hex::decode(&private_key).is_ok());
        assert!(hex::decode(&public_key).is_ok());
    }

    #[test]
    fn test_signature_verifier_creation() {
        let (_, public_key) = generate_keypair();
        let verifier = SignatureVerifier::new(&public_key);
        assert!(verifier.is_ok());

        let verifier = verifier.unwrap();
        assert_eq!(verifier.public_key_hex(), public_key);
    }

    #[test]
    fn test_signature_verifier_invalid_key() {
        // Empty key
        let result = SignatureVerifier::new("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));

        // Invalid hex
        let result = SignatureVerifier::new("invalid_hex");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid hex"));

        // Wrong length
        let result = SignatureVerifier::new("deadbeef");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be 32 bytes"));
    }

    #[test]
    fn test_sign_and_verify() {
        let test_data = b"Hello, world! This is test data for signature verification.";
        let (private_key, public_key) = generate_keypair();

        // Sign the data
        let signature = sign_data(test_data, &private_key).unwrap();
        assert_eq!(signature.len(), 128); // 64 bytes * 2 (hex)

        // Verify the signature
        let verifier = SignatureVerifier::new(&public_key).unwrap();
        let result = verifier.verify(&Bytes::from(test_data.to_vec()), &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_invalid_signature() {
        let test_data = b"Hello, world!";
        let (_, public_key) = generate_keypair();
        let verifier = SignatureVerifier::new(&public_key).unwrap();

        // Invalid hex signature
        let result = verifier.verify(&Bytes::from(test_data.to_vec()), "invalid_hex");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid hex signature"));

        // Wrong length signature
        let result = verifier.verify(&Bytes::from(test_data.to_vec()), "deadbeef");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("must be 64 bytes"));

        // Valid hex but wrong signature
        let fake_signature = "a".repeat(128);
        let result = verifier.verify(&Bytes::from(test_data.to_vec()), &fake_signature);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("verification failed"));
    }

    #[test]
    fn test_verify_wrong_data() {
        let original_data = b"Original data";
        let modified_data = b"Modified data";
        let (private_key, public_key) = generate_keypair();

        // Sign original data
        let signature = sign_data(original_data, &private_key).unwrap();

        // Try to verify with modified data
        let verifier = SignatureVerifier::new(&public_key).unwrap();
        let result = verifier.verify(&Bytes::from(modified_data.to_vec()), &signature);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("verification failed"));
    }

    #[test]
    fn test_verify_hash_direct() {
        let test_data = b"Test data for hash verification";
        let hash = Sha256::digest(test_data);
        let (private_key, public_key) = generate_keypair();

        // Sign the data (which internally hashes it)
        let signature = sign_data(test_data, &private_key).unwrap();

        // Verify using the hash directly
        let verifier = SignatureVerifier::new(&public_key).unwrap();
        let result = verifier.verify_hash(&hash, &signature);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cross_verification() {
        // Test that signatures from one key don't verify with another key
        let test_data = b"Cross verification test";
        let (private_key1, public_key1) = generate_keypair();
        let (_, public_key2) = generate_keypair();

        let signature = sign_data(test_data, &private_key1).unwrap();

        // Should verify with correct public key
        let verifier1 = SignatureVerifier::new(&public_key1).unwrap();
        assert!(verifier1
            .verify(&Bytes::from(test_data.to_vec()), &signature)
            .is_ok());

        // Should not verify with wrong public key
        let verifier2 = SignatureVerifier::new(&public_key2).unwrap();
        assert!(verifier2
            .verify(&Bytes::from(test_data.to_vec()), &signature)
            .is_err());
    }
}
