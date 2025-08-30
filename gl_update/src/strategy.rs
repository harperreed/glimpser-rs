//! ABOUTME: Update strategies for binary replacement and rollback
//! ABOUTME: Implements sidecar and self-replace update mechanisms

use crate::UpdateConfig;
use async_trait::async_trait;
use bytes::Bytes;
use gl_core::Result;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tracing::{debug, error, info, warn};

/// Trait for different update strategies
#[async_trait]
pub trait UpdateStrategy: Send + Sync {
    /// Install an update using this strategy
    async fn install_update(&self, binary_data: Bytes) -> Result<()>;

    /// Rollback to previous version
    async fn rollback(&self) -> Result<()>;

    /// Get strategy name
    fn name(&self) -> &'static str;
}

/// Enum-based strategy to avoid dyn trait issues
pub enum UpdateStrategyImpl {
    Sidecar(SidecarStrategy),
    #[cfg(feature = "self-replace")]
    SelfReplace(SelfReplaceStrategy),
}

#[async_trait]
impl UpdateStrategy for UpdateStrategyImpl {
    async fn install_update(&self, binary_data: Bytes) -> Result<()> {
        match self {
            Self::Sidecar(strategy) => strategy.install_update(binary_data).await,
            #[cfg(feature = "self-replace")]
            Self::SelfReplace(strategy) => strategy.install_update(binary_data).await,
        }
    }

    async fn rollback(&self) -> Result<()> {
        match self {
            Self::Sidecar(strategy) => strategy.rollback().await,
            #[cfg(feature = "self-replace")]
            Self::SelfReplace(strategy) => strategy.rollback().await,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Sidecar(strategy) => strategy.name(),
            #[cfg(feature = "self-replace")]
            Self::SelfReplace(strategy) => strategy.name(),
        }
    }
}

/// Sidecar update strategy - safer approach using atomic symlink swaps
#[derive(Debug)]
pub struct SidecarStrategy {
    #[allow(dead_code)]
    config: UpdateConfig,
    current_binary_path: PathBuf,
    backup_path: PathBuf,
    staging_path: PathBuf,
}

impl SidecarStrategy {
    pub fn new(config: UpdateConfig) -> Result<Self> {
        let current_binary_path = config.install_dir.join(&config.binary_name);
        let backup_path = config
            .install_dir
            .join(format!("{}.backup", config.binary_name));
        let staging_path = config
            .install_dir
            .join(format!("{}.staging", config.binary_name));

        // Verify install directory exists and is writable
        if !config.install_dir.exists() {
            return Err(gl_core::Error::Configuration(format!(
                "Install directory does not exist: {}",
                config.install_dir.display()
            )));
        }

        // Test write permissions by creating a temp file
        let test_file = config.install_dir.join(".update_test");
        if let Err(e) = std::fs::write(&test_file, b"test") {
            return Err(gl_core::Error::Configuration(format!(
                "Install directory is not writable: {}",
                e
            )));
        }
        let _ = std::fs::remove_file(test_file);

        Ok(Self {
            config,
            current_binary_path,
            backup_path,
            staging_path,
        })
    }
}

#[async_trait]
impl UpdateStrategy for SidecarStrategy {
    async fn install_update(&self, binary_data: Bytes) -> Result<()> {
        info!("Installing update using sidecar strategy");

        // Step 1: Write new binary to staging area
        debug!(
            "Writing new binary to staging: {}",
            self.staging_path.display()
        );
        self.write_binary_atomically(&self.staging_path, &binary_data)
            .await?;

        // Step 2: Make it executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&self.staging_path)
                .map_err(|e| {
                    gl_core::Error::External(format!("Failed to get file permissions: {}", e))
                })?
                .permissions();
            perms.set_mode(0o755); // rwxr-xr-x
            std::fs::set_permissions(&self.staging_path, perms).map_err(|e| {
                gl_core::Error::External(format!("Failed to set executable permissions: {}", e))
            })?;
        }

        // Step 3: Create backup of current binary if it exists
        if self.current_binary_path.exists() {
            debug!("Creating backup of current binary");
            if let Err(e) = std::fs::copy(&self.current_binary_path, &self.backup_path) {
                // Clean up staging file
                let _ = std::fs::remove_file(&self.staging_path);
                return Err(gl_core::Error::External(format!(
                    "Failed to create backup: {}",
                    e
                )));
            }
        }

        // Step 4: Atomic swap - move staging to current
        debug!("Performing atomic swap");
        if let Err(e) = std::fs::rename(&self.staging_path, &self.current_binary_path) {
            error!("Atomic swap failed: {}", e);

            // Attempt to restore backup if it exists
            if self.backup_path.exists() {
                warn!("Attempting to restore from backup");
                if let Err(restore_err) =
                    std::fs::rename(&self.backup_path, &self.current_binary_path)
                {
                    error!("Failed to restore backup: {}", restore_err);
                }
            }

            return Err(gl_core::Error::External(format!(
                "Failed to perform atomic swap: {}",
                e
            )));
        }

        info!("Update installation completed successfully");
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        info!("Rolling back using sidecar strategy");

        if !self.backup_path.exists() {
            return Err(gl_core::Error::External(
                "No backup available for rollback".to_string(),
            ));
        }

        // Move backup back to current
        std::fs::rename(&self.backup_path, &self.current_binary_path)
            .map_err(|e| gl_core::Error::External(format!("Rollback failed: {}", e)))?;

        info!("Rollback completed successfully");
        Ok(())
    }

    fn name(&self) -> &'static str {
        "sidecar"
    }
}

impl SidecarStrategy {
    async fn write_binary_atomically(&self, target_path: &Path, data: &Bytes) -> Result<()> {
        // Create temporary file in the same directory to ensure atomic move
        let temp_dir = target_path
            .parent()
            .ok_or_else(|| gl_core::Error::Configuration("Invalid target path".to_string()))?;

        let temp_file = NamedTempFile::new_in(temp_dir)
            .map_err(|e| gl_core::Error::External(format!("Failed to create temp file: {}", e)))?;

        // Write data to temp file
        tokio::fs::write(temp_file.path(), data)
            .await
            .map_err(|e| gl_core::Error::External(format!("Failed to write temp file: {}", e)))?;

        // Sync to disk
        let file = tokio::fs::File::open(temp_file.path()).await.map_err(|e| {
            gl_core::Error::External(format!("Failed to open temp file for sync: {}", e))
        })?;
        file.sync_all()
            .await
            .map_err(|e| gl_core::Error::External(format!("Failed to sync temp file: {}", e)))?;

        // Atomically move to target
        temp_file
            .persist(target_path)
            .map_err(|e| gl_core::Error::External(format!("Failed to persist temp file: {}", e)))?;

        Ok(())
    }
}

/// Self-replace update strategy - more dangerous but doesn't require external process
#[cfg(feature = "self-replace")]
#[derive(Debug)]
pub struct SelfReplaceStrategy {
    config: UpdateConfig,
    current_exe: PathBuf,
}

#[cfg(feature = "self-replace")]
impl SelfReplaceStrategy {
    pub fn new(config: UpdateConfig) -> Result<Self> {
        let current_exe = std::env::current_exe().map_err(|e| {
            gl_core::Error::External(format!("Failed to get current executable path: {}", e))
        })?;

        info!("Self-replace strategy for: {}", current_exe.display());

        Ok(Self {
            config,
            current_exe,
        })
    }
}

#[cfg(feature = "self-replace")]
#[async_trait]
impl UpdateStrategy for SelfReplaceStrategy {
    async fn install_update(&self, binary_data: Bytes) -> Result<()> {
        warn!("Installing update using self-replace strategy - use with caution!");

        // Create backup
        let backup_path = self.current_exe.with_extension("backup");
        debug!("Creating backup at: {}", backup_path.display());

        tokio::fs::copy(&self.current_exe, &backup_path)
            .await
            .map_err(|e| gl_core::Error::External(format!("Failed to create backup: {}", e)))?;

        // Write new binary to temp file first
        let temp_path = self.current_exe.with_extension("tmp");
        tokio::fs::write(&temp_path, &binary_data)
            .await
            .map_err(|e| gl_core::Error::External(format!("Failed to write temp binary: {}", e)))?;

        // Make executable
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&temp_path)
                .map_err(|e| {
                    gl_core::Error::External(format!("Failed to get temp file permissions: {}", e))
                })?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&temp_path, perms).map_err(|e| {
                gl_core::Error::External(format!("Failed to set temp file permissions: {}", e))
            })?;
        }

        // Replace current executable
        #[cfg(unix)]
        {
            // On Unix, we can replace the running executable
            tokio::fs::rename(&temp_path, &self.current_exe)
                .await
                .map_err(|e| {
                    gl_core::Error::External(format!("Failed to replace executable: {}", e))
                })?;
        }

        #[cfg(windows)]
        {
            // On Windows, we need to use a different approach
            // Move current exe to .old, move new exe to current name
            let old_path = self.current_exe.with_extension("old");

            tokio::fs::rename(&self.current_exe, &old_path)
                .await
                .map_err(|e| {
                    gl_core::Error::External(format!("Failed to move current executable: {}", e))
                })?;

            if let Err(e) = tokio::fs::rename(&temp_path, &self.current_exe).await {
                // Restore original if replacement failed
                let _ = tokio::fs::rename(&old_path, &self.current_exe).await;
                return Err(gl_core::Error::External(format!(
                    "Failed to replace executable: {}",
                    e
                )));
            }

            // Schedule cleanup of .old file on next boot
            let _ = std::fs::remove_file(old_path);
        }

        info!("Self-replace update completed");
        Ok(())
    }

    async fn rollback(&self) -> Result<()> {
        info!("Rolling back using self-replace strategy");

        let backup_path = self.current_exe.with_extension("backup");
        if !backup_path.exists() {
            return Err(gl_core::Error::External(
                "No backup available for rollback".to_string(),
            ));
        }

        // Replace with backup
        tokio::fs::copy(&backup_path, &self.current_exe)
            .await
            .map_err(|e| gl_core::Error::External(format!("Rollback failed: {}", e)))?;

        info!("Self-replace rollback completed");
        Ok(())
    }

    fn name(&self) -> &'static str {
        "self-replace"
    }
}

/// Create an update strategy based on configuration
pub fn create_strategy(config: UpdateConfig) -> Result<UpdateStrategyImpl> {
    match config.strategy {
        crate::UpdateStrategyType::Sidecar => {
            #[cfg(feature = "sidecar")]
            {
                Ok(UpdateStrategyImpl::Sidecar(SidecarStrategy::new(config)?))
            }
            #[cfg(not(feature = "sidecar"))]
            {
                Err(gl_core::Error::Configuration(
                    "Sidecar strategy not compiled in".to_string(),
                ))
            }
        }
        crate::UpdateStrategyType::SelfReplace => {
            #[cfg(feature = "self-replace")]
            {
                Ok(UpdateStrategyImpl::SelfReplace(SelfReplaceStrategy::new(
                    config,
                )?))
            }
            #[cfg(not(feature = "self-replace"))]
            {
                Err(gl_core::Error::Configuration(
                    "Self-replace strategy not compiled in".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_sidecar_strategy_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = UpdateConfig {
            install_dir: temp_dir.path().to_path_buf(),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        let strategy = SidecarStrategy::new(config);
        assert!(strategy.is_ok());
        assert_eq!(strategy.unwrap().name(), "sidecar");
    }

    #[tokio::test]
    async fn test_sidecar_strategy_invalid_dir() {
        let config = UpdateConfig {
            install_dir: PathBuf::from("/nonexistent/directory"),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        let strategy = SidecarStrategy::new(config);
        assert!(strategy.is_err());
        assert!(strategy.unwrap_err().to_string().contains("does not exist"));
    }

    #[tokio::test]
    async fn test_sidecar_install_new_binary() {
        let temp_dir = TempDir::new().unwrap();
        let config = UpdateConfig {
            install_dir: temp_dir.path().to_path_buf(),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        let strategy = SidecarStrategy::new(config).unwrap();
        let test_data = Bytes::from("fake binary data");

        let result = strategy.install_update(test_data.clone()).await;
        assert!(result.is_ok());

        // Check that binary was written
        let binary_path = temp_dir.path().join("test_app");
        assert!(binary_path.exists());

        let written_data = fs::read(&binary_path).unwrap();
        assert_eq!(written_data, test_data.as_ref());

        // Check permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&binary_path).unwrap();
            let permissions = metadata.permissions();
            assert_eq!(permissions.mode() & 0o777, 0o755);
        }
    }

    #[tokio::test]
    async fn test_sidecar_install_with_backup() {
        let temp_dir = TempDir::new().unwrap();
        let config = UpdateConfig {
            install_dir: temp_dir.path().to_path_buf(),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        // Create existing binary
        let binary_path = temp_dir.path().join("test_app");
        let original_data = b"original binary";
        fs::write(&binary_path, original_data).unwrap();

        let strategy = SidecarStrategy::new(config).unwrap();
        let new_data = Bytes::from("new binary data");

        let result = strategy.install_update(new_data.clone()).await;
        assert!(result.is_ok());

        // Check that new binary was written
        let written_data = fs::read(&binary_path).unwrap();
        assert_eq!(written_data, new_data.as_ref());

        // Check that backup was created
        let backup_path = temp_dir.path().join("test_app.backup");
        assert!(backup_path.exists());
        let backup_data = fs::read(&backup_path).unwrap();
        assert_eq!(backup_data, original_data);
    }

    #[tokio::test]
    async fn test_sidecar_rollback() {
        let temp_dir = TempDir::new().unwrap();
        let config = UpdateConfig {
            install_dir: temp_dir.path().to_path_buf(),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        // Create backup file
        let backup_path = temp_dir.path().join("test_app.backup");
        let backup_data = b"backup binary data";
        fs::write(&backup_path, backup_data).unwrap();

        // Create current binary with different data
        let binary_path = temp_dir.path().join("test_app");
        let current_data = b"current binary data";
        fs::write(&binary_path, current_data).unwrap();

        let strategy = SidecarStrategy::new(config).unwrap();

        let result = strategy.rollback().await;
        assert!(result.is_ok());

        // Check that backup was restored
        let restored_data = fs::read(&binary_path).unwrap();
        assert_eq!(restored_data, backup_data);

        // Check that backup file was moved (no longer exists)
        assert!(!backup_path.exists());
    }

    #[tokio::test]
    async fn test_sidecar_rollback_no_backup() {
        let temp_dir = TempDir::new().unwrap();
        let config = UpdateConfig {
            install_dir: temp_dir.path().to_path_buf(),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        let strategy = SidecarStrategy::new(config).unwrap();

        let result = strategy.rollback().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No backup available"));
    }

    #[tokio::test]
    async fn test_write_binary_atomically() {
        let temp_dir = TempDir::new().unwrap();
        let config = UpdateConfig {
            install_dir: temp_dir.path().to_path_buf(),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        let strategy = SidecarStrategy::new(config).unwrap();
        let target_path = temp_dir.path().join("atomic_test");
        let test_data = Bytes::from("atomic write test data");

        let result = strategy
            .write_binary_atomically(&target_path, &test_data)
            .await;
        assert!(result.is_ok());

        assert!(target_path.exists());
        let written_data = fs::read(&target_path).unwrap();
        assert_eq!(written_data, test_data.as_ref());
    }

    #[test]
    fn test_create_strategy_sidecar() {
        let temp_dir = TempDir::new().unwrap();
        let config = UpdateConfig {
            strategy: crate::UpdateStrategyType::Sidecar,
            install_dir: temp_dir.path().to_path_buf(),
            binary_name: "test_app".to_string(),
            ..Default::default()
        };

        let strategy = create_strategy(config);
        assert!(strategy.is_ok());
        assert_eq!(strategy.unwrap().name(), "sidecar");
    }

    #[cfg(feature = "self-replace")]
    #[test]
    fn test_create_strategy_self_replace() {
        let config = UpdateConfig {
            strategy: crate::UpdateStrategyType::SelfReplace,
            ..Default::default()
        };

        let strategy = create_strategy(config);
        assert!(strategy.is_ok());
        assert_eq!(strategy.unwrap().name(), "self-replace");
    }
}
