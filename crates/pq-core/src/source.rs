use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use futures::stream::BoxStream;
use object_store::path::Path as ObjectPath;
use object_store::{
    ClientOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOpts, PutOptions, PutPayload, PutResult,
};
use url::Url;

use crate::error::{PqError, Result};

// ---------------------------------------------------------------------------
// Global debug flag
// ---------------------------------------------------------------------------

static DEBUG_HTTP: AtomicBool = AtomicBool::new(false);

/// Enable or disable HTTP request debug logging to stderr.
pub fn set_debug(enabled: bool) {
    DEBUG_HTTP.store(enabled, Ordering::Relaxed);
}

fn is_debug() -> bool {
    DEBUG_HTTP.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// ObjectStore cache — reuse HTTP clients / TCP connections across calls
// ---------------------------------------------------------------------------

fn store_cache() -> &'static Mutex<HashMap<String, Arc<dyn ObjectStore>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<dyn ObjectStore>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// URL helpers
// ---------------------------------------------------------------------------

/// Returns true if the location string looks like a remote URL.
pub fn is_url(location: &str) -> bool {
    location.starts_with("http://")
        || location.starts_with("https://")
        || location.starts_with("s3://")
        || location.starts_with("gs://")
        || location.starts_with("az://")
        || location.starts_with("abfss://")
}

/// Parse a URL string into an ObjectStore instance and a path within that store.
/// The store is cached by base URL so repeated calls reuse the same HTTP client.
pub fn parse_url(location: &str) -> Result<(Arc<dyn ObjectStore>, ObjectPath)> {
    let url = Url::parse(location)
        .map_err(|e| PqError::Other(format!("Invalid URL '{location}': {e}")))?;

    let (base_key, obj_path) = match url.scheme() {
        "http" | "https" => {
            let host = url
                .host_str()
                .ok_or_else(|| PqError::Other(format!("URL has no host: {location}")))?;
            let port_suffix = url.port().map(|p| format!(":{p}")).unwrap_or_default();
            let base = format!("{}://{host}{port_suffix}", url.scheme());
            let path = url.path().trim_start_matches('/');
            (base, ObjectPath::from(path))
        }
        "s3" => {
            let bucket = url
                .host_str()
                .ok_or_else(|| PqError::Other(format!("S3 URL has no bucket: {location}")))?;
            let base = format!("s3://{bucket}");
            let path = url.path().trim_start_matches('/');
            (base, ObjectPath::from(path))
        }
        "gs" => {
            let bucket = url
                .host_str()
                .ok_or_else(|| PqError::Other(format!("GCS URL has no bucket: {location}")))?;
            let base = format!("gs://{bucket}");
            let path = url.path().trim_start_matches('/');
            (base, ObjectPath::from(path))
        }
        "az" | "abfss" => {
            let container = url
                .host_str()
                .ok_or_else(|| PqError::Other(format!("Azure URL has no container: {location}")))?;
            let base = format!("{}://{container}", url.scheme());
            let path = url.path().trim_start_matches('/');
            (base, ObjectPath::from(path))
        }
        scheme => {
            return Err(PqError::Other(format!(
                "Unsupported URL scheme: {scheme}://"
            )));
        }
    };

    // Return cached store if available
    {
        let guard = store_cache().lock().unwrap();
        if let Some(store) = guard.get(&base_key) {
            return Ok((store.clone(), obj_path));
        }
    }

    // Build new store
    let store: Arc<dyn ObjectStore> = match url.scheme() {
        "http" | "https" => {
            let client_options = ClientOptions::new().with_allow_http(true);
            let store = object_store::http::HttpBuilder::new()
                .with_url(&base_key)
                .with_client_options(client_options)
                .build()
                .map_err(|e| PqError::ObjectStore(e.to_string()))?;
            wrap_store(Arc::new(store))
        }
        "s3" => {
            let bucket = url.host_str().unwrap();
            let store = object_store::aws::AmazonS3Builder::from_env()
                .with_bucket_name(bucket)
                .build()
                .map_err(|e| PqError::ObjectStore(e.to_string()))?;
            wrap_store(Arc::new(store))
        }
        "gs" => {
            let bucket = url.host_str().unwrap();
            let store = object_store::gcp::GoogleCloudStorageBuilder::from_env()
                .with_bucket_name(bucket)
                .build()
                .map_err(|e| PqError::ObjectStore(e.to_string()))?;
            wrap_store(Arc::new(store))
        }
        "az" | "abfss" => {
            let container = url.host_str().unwrap();
            let store = object_store::azure::MicrosoftAzureBuilder::from_env()
                .with_container_name(container)
                .build()
                .map_err(|e| PqError::ObjectStore(e.to_string()))?;
            wrap_store(Arc::new(store))
        }
        _ => unreachable!(),
    };

    store_cache()
        .lock()
        .unwrap()
        .insert(base_key, store.clone());
    Ok((store, obj_path))
}

/// Run an async future, handling the case where we may or may not already
/// be inside a tokio runtime.
pub fn block_on_async<F: std::future::Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(f)),
        Err(_) => {
            let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(f)
        }
    }
}

// ---------------------------------------------------------------------------
// Logging ObjectStore wrapper
// ---------------------------------------------------------------------------

/// Optionally wrap a store with debug logging.
fn wrap_store(store: Arc<dyn ObjectStore>) -> Arc<dyn ObjectStore> {
    if is_debug() {
        Arc::new(LoggingStore { inner: store })
    } else {
        store
    }
}

#[derive(Debug)]
struct LoggingStore {
    inner: Arc<dyn ObjectStore>,
}

impl fmt::Display for LoggingStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LoggingStore({})", self.inner)
    }
}

#[async_trait]
impl ObjectStore for LoggingStore {
    async fn put_opts(
        &self,
        location: &ObjectPath,
        payload: PutPayload,
        opts: PutOptions,
    ) -> object_store::Result<PutResult> {
        self.inner.put_opts(location, payload, opts).await
    }

    async fn put_multipart_opts(
        &self,
        location: &ObjectPath,
        opts: PutMultipartOpts,
    ) -> object_store::Result<Box<dyn MultipartUpload>> {
        self.inner.put_multipart_opts(location, opts).await
    }

    async fn get_opts(
        &self,
        location: &ObjectPath,
        options: GetOptions,
    ) -> object_store::Result<GetResult> {
        let is_head = options.head;
        let range = options.range.clone();
        let start = std::time::Instant::now();
        let result = self.inner.get_opts(location, options).await;
        let elapsed = start.elapsed();
        match &result {
            Ok(r) => {
                let method = if is_head { "HEAD" } else { "GET" };
                let range_str = match &range {
                    Some(r) => format!("{r:?}"),
                    None => "full".to_string(),
                };
                eprintln!(
                    "[debug] {method} {location}  range={range_str}  file_size={}  {elapsed:.1?}",
                    bytesize::ByteSize(r.meta.size as u64),
                );
            }
            Err(e) => {
                let method = if is_head { "HEAD" } else { "GET" };
                eprintln!("[debug] {method} {location}  ERROR={e}  {elapsed:.1?}");
            }
        }
        result
    }

    async fn delete(&self, location: &ObjectPath) -> object_store::Result<()> {
        self.inner.delete(location).await
    }

    fn list(&self, prefix: Option<&ObjectPath>) -> BoxStream<'_, object_store::Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    async fn list_with_delimiter(
        &self,
        prefix: Option<&ObjectPath>,
    ) -> object_store::Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy(&self, from: &ObjectPath, to: &ObjectPath) -> object_store::Result<()> {
        self.inner.copy(from, to).await
    }

    async fn copy_if_not_exists(
        &self,
        from: &ObjectPath,
        to: &ObjectPath,
    ) -> object_store::Result<()> {
        self.inner.copy_if_not_exists(from, to).await
    }
}
