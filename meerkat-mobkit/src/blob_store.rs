//! MobKit-owned binary blob storage.
//!
//! Published Meerkat 0.6 exposes a `BlobStore` trait whose payload is base64
//! text. MobKit keeps that compatibility seam for the runtime while storing
//! local blobs as raw bytes for HTTP serving and multipart upload paths.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine;
use bytes::Bytes;
use meerkat_core::{BlobId, BlobPayload, BlobRef, BlobStore, BlobStoreError};
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, ObjectStoreExt, PutPayload};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryBlobPayload {
    pub blob_id: BlobId,
    pub media_type: String,
    pub size: u64,
    pub data: Bytes,
}

/// Which typed form a caller injected the blob slot in (M4b).
///
/// MobKit needs both faces of one store — the raw-bytes
/// [`BinaryBlobStore`] (HTTP `/blobs/{id}` serving, `mobkit/blob/*`) and
/// meerkat's base64 `BlobStore` (runtime image blobs). A caller who has the
/// binary form injects it directly and avoids the
/// [`BinaryBlobStoreAdapter`] base64 round-trip on the byte-serving paths;
/// a caller with only the meerkat form keeps the historical adapter shape.
#[derive(Clone)]
pub enum BlobStoreInjection {
    /// A meerkat `BlobStore` (base64 payloads); the binary face is adapted.
    Core(Arc<dyn BlobStore>),
    /// A raw-bytes [`BinaryBlobStore`]; the meerkat face is adapted.
    Binary(Arc<dyn BinaryBlobStore>),
}

impl BlobStoreInjection {
    /// Resolve both faces of the injected store.
    pub(crate) fn into_pair(self) -> (Arc<dyn BinaryBlobStore>, Arc<dyn BlobStore>) {
        match self {
            Self::Core(core) => (Arc::new(BinaryBlobStoreAdapter::new(core.clone())), core),
            Self::Binary(binary) => (
                binary.clone(),
                Arc::new(Base64BlobStoreAdapter::new(binary)),
            ),
        }
    }
}

#[async_trait]
pub trait BinaryBlobStore: Send + Sync {
    async fn put_bytes(&self, media_type: &str, data: Bytes) -> Result<BlobRef, BlobStoreError>;
    async fn get_bytes(&self, blob_id: &BlobId) -> Result<BinaryBlobPayload, BlobStoreError>;
    async fn delete(&self, blob_id: &BlobId) -> Result<(), BlobStoreError>;
    fn is_persistent(&self) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlobMetadata {
    media_type: String,
    size: u64,
    created_at_ms: u64,
    source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyStoredBlob {
    media_type: String,
    data: String,
}

#[derive(Debug)]
enum BlobObjectBackend {
    ObjectStore {
        store: Arc<dyn ObjectStore>,
        persistent: bool,
    },
    Memory {
        blobs: RwLock<std::collections::BTreeMap<BlobId, (String, Bytes)>>,
    },
}

pub struct ObjectStoreBlobStore {
    backend: BlobObjectBackend,
    legacy_root: Option<PathBuf>,
}

impl ObjectStoreBlobStore {
    pub fn local(root: PathBuf) -> Result<Self, BlobStoreError> {
        std::fs::create_dir_all(&root).map_err(|err| BlobStoreError::Internal(err.to_string()))?;
        let store = object_store::local::LocalFileSystem::new_with_prefix(&root)
            .map_err(|err| BlobStoreError::Internal(err.to_string()))?;
        Ok(Self {
            backend: BlobObjectBackend::ObjectStore {
                store: Arc::new(store),
                persistent: true,
            },
            legacy_root: Some(root),
        })
    }

    pub fn memory() -> Self {
        Self {
            backend: BlobObjectBackend::Memory {
                blobs: RwLock::new(std::collections::BTreeMap::new()),
            },
            legacy_root: None,
        }
    }

    fn object_path(blob_id: &BlobId) -> ObjectPath {
        ObjectPath::from(format!("objects/{}.bin", storage_key(blob_id)))
    }

    fn meta_path(blob_id: &BlobId) -> ObjectPath {
        ObjectPath::from(format!("meta/{}.json", storage_key(blob_id)))
    }

    fn legacy_path(&self, blob_id: &BlobId) -> Option<PathBuf> {
        let root = self.legacy_root.as_ref()?;
        let key = storage_key(blob_id);
        let prefix = key.get(0..2).unwrap_or("xx");
        Some(root.join(prefix).join(format!("{key}.json")))
    }

    async fn read_legacy(&self, blob_id: &BlobId) -> Result<BinaryBlobPayload, BlobStoreError> {
        let path = self
            .legacy_path(blob_id)
            .ok_or_else(|| BlobStoreError::NotFound(blob_id.clone()))?;
        let bytes = tokio::fs::read(&path).await.map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                BlobStoreError::NotFound(blob_id.clone())
            } else {
                BlobStoreError::ReadFailed(err.to_string())
            }
        })?;
        let stored: LegacyStoredBlob = serde_json::from_slice(&bytes)
            .map_err(|err| BlobStoreError::ReadFailed(err.to_string()))?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(stored.data.as_bytes())
            .map_err(|err| {
                BlobStoreError::ReadFailed(format!("invalid legacy blob base64: {err}"))
            })?;
        Ok(BinaryBlobPayload {
            blob_id: blob_id.clone(),
            media_type: stored.media_type,
            size: decoded.len() as u64,
            data: Bytes::from(decoded),
        })
    }
}

#[async_trait]
impl BinaryBlobStore for ObjectStoreBlobStore {
    async fn put_bytes(&self, media_type: &str, data: Bytes) -> Result<BlobRef, BlobStoreError> {
        let blob_id = compute_blob_id(media_type, &data);
        match &self.backend {
            BlobObjectBackend::ObjectStore { store, .. } => {
                let meta = BlobMetadata {
                    media_type: media_type.to_string(),
                    size: data.len() as u64,
                    created_at_ms: current_time_ms(),
                    source: "mobkit".to_string(),
                };
                let meta_bytes = serde_json::to_vec(&meta)
                    .map_err(|err| BlobStoreError::WriteFailed(err.to_string()))?;
                store
                    .put(&Self::object_path(&blob_id), PutPayload::from(data.clone()))
                    .await
                    .map_err(|err| BlobStoreError::WriteFailed(err.to_string()))?;
                store
                    .put(
                        &Self::meta_path(&blob_id),
                        PutPayload::from(Bytes::from(meta_bytes)),
                    )
                    .await
                    .map_err(|err| BlobStoreError::WriteFailed(err.to_string()))?;
            }
            BlobObjectBackend::Memory { blobs } => {
                blobs
                    .write()
                    .await
                    .entry(blob_id.clone())
                    .or_insert_with(|| (media_type.to_string(), data.clone()));
            }
        }
        Ok(BlobRef {
            blob_id,
            media_type: media_type.to_string(),
        })
    }

    async fn get_bytes(&self, blob_id: &BlobId) -> Result<BinaryBlobPayload, BlobStoreError> {
        if !is_valid_blob_id(blob_id) {
            return Err(BlobStoreError::NotFound(blob_id.clone()));
        }
        match &self.backend {
            BlobObjectBackend::ObjectStore { store, .. } => {
                let meta_bytes = match store.get(&Self::meta_path(blob_id)).await {
                    Ok(result) => result
                        .bytes()
                        .await
                        .map_err(|err| BlobStoreError::ReadFailed(err.to_string()))?,
                    Err(object_store::Error::NotFound { .. }) => {
                        return self.read_legacy(blob_id).await;
                    }
                    Err(err) => return Err(BlobStoreError::ReadFailed(err.to_string())),
                };
                let meta: BlobMetadata = serde_json::from_slice(&meta_bytes)
                    .map_err(|err| BlobStoreError::ReadFailed(err.to_string()))?;
                let data = match store.get(&Self::object_path(blob_id)).await {
                    Ok(result) => result
                        .bytes()
                        .await
                        .map_err(|err| BlobStoreError::ReadFailed(err.to_string()))?,
                    Err(object_store::Error::NotFound { .. }) => {
                        return self.read_legacy(blob_id).await;
                    }
                    Err(err) => return Err(BlobStoreError::ReadFailed(err.to_string())),
                };
                Ok(BinaryBlobPayload {
                    blob_id: blob_id.clone(),
                    media_type: meta.media_type,
                    size: meta.size,
                    data,
                })
            }
            BlobObjectBackend::Memory { blobs } => {
                let blobs = blobs.read().await;
                let (media_type, data) = blobs
                    .get(blob_id)
                    .ok_or_else(|| BlobStoreError::NotFound(blob_id.clone()))?;
                Ok(BinaryBlobPayload {
                    blob_id: blob_id.clone(),
                    media_type: media_type.clone(),
                    size: data.len() as u64,
                    data: data.clone(),
                })
            }
        }
    }

    async fn delete(&self, blob_id: &BlobId) -> Result<(), BlobStoreError> {
        if !is_valid_blob_id(blob_id) {
            return Err(BlobStoreError::NotFound(blob_id.clone()));
        }
        match &self.backend {
            BlobObjectBackend::ObjectStore { store, .. } => {
                for path in [Self::object_path(blob_id), Self::meta_path(blob_id)] {
                    match store.delete(&path).await {
                        Ok(()) => {}
                        Err(object_store::Error::NotFound { .. }) => {}
                        Err(err) => return Err(BlobStoreError::DeleteFailed(err.to_string())),
                    }
                }
            }
            BlobObjectBackend::Memory { blobs } => {
                blobs.write().await.remove(blob_id);
            }
        }
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        match &self.backend {
            BlobObjectBackend::ObjectStore { persistent, .. } => *persistent,
            BlobObjectBackend::Memory { .. } => false,
        }
    }
}

pub struct Base64BlobStoreAdapter {
    inner: Arc<dyn BinaryBlobStore>,
}

impl Base64BlobStoreAdapter {
    pub fn new(inner: Arc<dyn BinaryBlobStore>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl BlobStore for Base64BlobStoreAdapter {
    async fn put_image(&self, media_type: &str, data: &str) -> Result<BlobRef, BlobStoreError> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .map_err(|err| BlobStoreError::WriteFailed(format!("invalid blob base64: {err}")))?;
        self.inner.put_bytes(media_type, Bytes::from(bytes)).await
    }

    async fn get(&self, blob_id: &BlobId) -> Result<BlobPayload, BlobStoreError> {
        let payload = self.inner.get_bytes(blob_id).await?;
        Ok(BlobPayload {
            blob_id: payload.blob_id,
            media_type: payload.media_type,
            data: base64::engine::general_purpose::STANDARD.encode(payload.data.as_ref()),
        })
    }

    async fn delete(&self, blob_id: &BlobId) -> Result<(), BlobStoreError> {
        self.inner.delete(blob_id).await
    }

    fn is_persistent(&self) -> bool {
        self.inner.is_persistent()
    }
}

pub struct BinaryBlobStoreAdapter {
    inner: Arc<dyn BlobStore>,
}

impl BinaryBlobStoreAdapter {
    pub fn new(inner: Arc<dyn BlobStore>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl BinaryBlobStore for BinaryBlobStoreAdapter {
    async fn put_bytes(&self, media_type: &str, data: Bytes) -> Result<BlobRef, BlobStoreError> {
        let encoded = base64::engine::general_purpose::STANDARD.encode(data.as_ref());
        self.inner.put_image(media_type, &encoded).await
    }

    async fn get_bytes(&self, blob_id: &BlobId) -> Result<BinaryBlobPayload, BlobStoreError> {
        let payload = self.inner.get(blob_id).await?;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload.data.as_bytes())
            .map_err(|err| {
                BlobStoreError::ReadFailed(format!("invalid blob base64 payload: {err}"))
            })?;
        Ok(BinaryBlobPayload {
            blob_id: payload.blob_id,
            media_type: payload.media_type,
            size: decoded.len() as u64,
            data: Bytes::from(decoded),
        })
    }

    async fn delete(&self, blob_id: &BlobId) -> Result<(), BlobStoreError> {
        self.inner.delete(blob_id).await
    }

    fn is_persistent(&self) -> bool {
        self.inner.is_persistent()
    }
}

fn compute_blob_id(media_type: &str, bytes: &[u8]) -> BlobId {
    let mut hasher = Sha256::new();
    hasher.update(media_type.as_bytes());
    hasher.update([0]);
    hasher.update(bytes);
    BlobId::new(format!("sha256:{:x}", hasher.finalize()))
}

pub fn is_valid_blob_id(blob_id: &BlobId) -> bool {
    is_valid_blob_id_value(blob_id.as_str())
}

pub fn is_valid_blob_id_value(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn storage_key(blob_id: &BlobId) -> &str {
    blob_id
        .as_str()
        .strip_prefix("sha256:")
        .unwrap_or(blob_id.as_str())
}

fn current_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn binary_blob_ids_hash_raw_bytes_not_base64_text() {
        let store = ObjectStoreBlobStore::memory();
        let first = store
            .put_bytes("image/png", Bytes::from_static(b"abc"))
            .await
            .expect("put");
        let second = store
            .put_bytes("image/png", Bytes::from_static(b"abc"))
            .await
            .expect("put same");
        let third = store
            .put_bytes("image/jpeg", Bytes::from_static(b"abc"))
            .await
            .expect("put media");

        assert_eq!(first.blob_id, second.blob_id);
        assert_ne!(first.blob_id, third.blob_id);
        assert_ne!(
            first.blob_id,
            compute_legacy_blob_id("image/png", "YWJj"),
            "raw-byte hashing must not reuse the legacy base64-text hash"
        );
    }

    #[tokio::test]
    async fn base64_adapter_roundtrips_at_json_boundary() {
        let binary: Arc<dyn BinaryBlobStore> = Arc::new(ObjectStoreBlobStore::memory());
        let adapter = Base64BlobStoreAdapter::new(binary);
        let blob = adapter
            .put_image("image/png", "YWJj")
            .await
            .expect("put base64");
        let payload = adapter.get(&blob.blob_id).await.expect("get base64");
        assert_eq!(payload.media_type, "image/png");
        assert_eq!(payload.data, "YWJj");
    }

    #[tokio::test]
    async fn local_store_reads_legacy_fs_blob_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = ObjectStoreBlobStore::local(temp.path().to_path_buf()).expect("local store");
        let legacy_id = compute_legacy_blob_id("image/png", "YWJj");
        let key = storage_key(&legacy_id);
        let legacy_dir = temp.path().join(&key[0..2]);
        tokio::fs::create_dir_all(&legacy_dir)
            .await
            .expect("legacy dir");
        tokio::fs::write(
            legacy_dir.join(format!("{key}.json")),
            serde_json::to_vec(&LegacyStoredBlob {
                media_type: "image/png".to_string(),
                data: "YWJj".to_string(),
            })
            .expect("legacy json"),
        )
        .await
        .expect("legacy write");

        let payload = store.get_bytes(&legacy_id).await.expect("legacy fallback");
        assert_eq!(payload.blob_id, legacy_id);
        assert_eq!(payload.media_type, "image/png");
        assert_eq!(payload.data, Bytes::from_static(b"abc"));
    }

    #[test]
    fn local_store_creates_missing_root() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join("missing").join("blobs");

        let store = ObjectStoreBlobStore::local(root.clone()).expect("local store opens");

        assert!(root.is_dir());
        assert!(store.is_persistent());
    }

    #[tokio::test]
    async fn invalid_blob_ids_do_not_address_object_paths() {
        let store = ObjectStoreBlobStore::memory();
        let invalid = BlobId::from("../not-a-sha");

        let err = store.get_bytes(&invalid).await.expect_err("invalid id");
        assert!(matches!(err, BlobStoreError::NotFound(id) if id == invalid));
        let err = store.delete(&invalid).await.expect_err("invalid delete");
        assert!(matches!(err, BlobStoreError::NotFound(id) if id == invalid));
    }

    fn compute_legacy_blob_id(media_type: &str, data: &str) -> BlobId {
        let mut hasher = Sha256::new();
        hasher.update(media_type.as_bytes());
        hasher.update([0]);
        hasher.update(data.as_bytes());
        BlobId::new(format!("sha256:{:x}", hasher.finalize()))
    }
}
