//! ABOUTME: Integration tests for storage operations with real I/O
//! ABOUTME: Tests actual file operations, network calls, and persistence

use bytes::Bytes;
use gl_storage::{Storage, StorageConfig, StorageManager, StorageUri};
use tempfile::TempDir;

#[tokio::test]
async fn test_local_storage_roundtrip() {
    let temp_dir = TempDir::new().unwrap();

    let config = StorageConfig {
        base_dir: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    };

    let storage = StorageManager::new(config).unwrap();
    let uri = StorageUri::new(format!("file://{}/test.txt", temp_dir.path().display())).unwrap();
    let test_data = Bytes::from("Hello, storage!");

    // Test put
    let result = storage.put(&uri, test_data.clone()).await.unwrap();
    assert_eq!(result.size, test_data.len());
    assert!(result.checksum.is_some());

    // Test exists
    assert!(storage.exists(&uri).await.unwrap());

    // Test get
    let retrieved = storage.get(&uri).await.unwrap();
    assert_eq!(retrieved, test_data);

    // Test metadata
    let metadata = storage.metadata(&uri).await.unwrap();
    assert_eq!(metadata.size, test_data.len());

    // Test delete
    storage.delete(&uri).await.unwrap();
    assert!(!storage.exists(&uri).await.unwrap());
}

#[tokio::test]
async fn test_storage_manager_creation_with_config() {
    let temp_dir = TempDir::new().unwrap();

    let config = StorageConfig {
        base_dir: Some(temp_dir.path().to_path_buf()),
        retry_attempts: 5,
        ..Default::default()
    };

    let storage = StorageManager::new(config);
    assert!(storage.is_ok());
}

#[tokio::test]
async fn test_large_file_operations() {
    let temp_dir = TempDir::new().unwrap();

    let config = StorageConfig {
        base_dir: Some(temp_dir.path().to_path_buf()),
        ..Default::default()
    };

    let storage = StorageManager::new(config).unwrap();
    let uri = StorageUri::new(format!(
        "file://{}/large_test.bin",
        temp_dir.path().display()
    ))
    .unwrap();

    // Create 1MB test data
    let test_data = Bytes::from(vec![0x42; 1024 * 1024]);

    // Test put large file
    let result = storage.put(&uri, test_data.clone()).await.unwrap();
    assert_eq!(result.size, test_data.len());

    // Test get large file
    let retrieved = storage.get(&uri).await.unwrap();
    assert_eq!(retrieved.len(), test_data.len());
    assert_eq!(retrieved, test_data);

    // Cleanup
    storage.delete(&uri).await.unwrap();
}
