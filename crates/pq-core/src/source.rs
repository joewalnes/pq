use std::sync::Arc;

use object_store::path::Path as ObjectPath;
use object_store::ClientOptions;
use object_store::ObjectStore;
use url::Url;

use crate::error::{PqError, Result};

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
pub fn parse_url(location: &str) -> Result<(Arc<dyn ObjectStore>, ObjectPath)> {
    let url = Url::parse(location)
        .map_err(|e| PqError::Other(format!("Invalid URL '{location}': {e}")))?;

    match url.scheme() {
        "http" | "https" => {
            let host = url
                .host_str()
                .ok_or_else(|| PqError::Other(format!("URL has no host: {location}")))?;
            let port_suffix = url.port().map(|p| format!(":{p}")).unwrap_or_default();
            let base = format!("{}://{host}{port_suffix}", url.scheme());

            let client_options = ClientOptions::new().with_allow_http(true);
            let store = object_store::http::HttpBuilder::new()
                .with_url(&base)
                .with_client_options(client_options)
                .build()
                .map_err(|e| PqError::ObjectStore(e.to_string()))?;

            let path = url.path().trim_start_matches('/');
            Ok((Arc::new(store), ObjectPath::from(path)))
        }
        "s3" => {
            let bucket = url
                .host_str()
                .ok_or_else(|| PqError::Other(format!("S3 URL has no bucket: {location}")))?;

            let store = object_store::aws::AmazonS3Builder::from_env()
                .with_bucket_name(bucket)
                .build()
                .map_err(|e| PqError::ObjectStore(e.to_string()))?;

            let path = url.path().trim_start_matches('/');
            Ok((Arc::new(store), ObjectPath::from(path)))
        }
        scheme => Err(PqError::Other(format!(
            "Unsupported URL scheme: {scheme}://"
        ))),
    }
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
