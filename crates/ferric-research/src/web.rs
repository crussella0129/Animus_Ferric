//! The Web Retriever plane.
use async_trait::async_trait;
use crate::retriever::{RetrievedChunk, Retriever, RetrieveError};
use crate::sandbox::{SandboxConfig, check_available, run_in_sandbox, SandboxError};

pub struct WebRetriever {
    config: SandboxConfig,
}

impl Default for WebRetriever {
    fn default() -> Self {
        Self::new()
    }
}

impl WebRetriever {
    pub fn new() -> Self {
        Self {
            config: SandboxConfig {
                image: "alpine:latest".to_string(),
                enforce_runsc: false,
                proxy_url: None,
            }
        }
    }

    pub fn with_runsc(mut self, enforce: bool) -> Self {
        self.config.enforce_runsc = enforce;
        self
    }

    pub fn with_proxy(mut self, proxy_url: Option<String>) -> Self {
        self.config.proxy_url = proxy_url;
        self
    }
}

#[async_trait]
impl Retriever for WebRetriever {
    fn plane(&self) -> &str {
        "web"
    }

    fn available(&self) -> bool {
        check_available()
    }

    async fn retrieve(&self, query: &str) -> Result<Vec<RetrievedChunk>, RetrieveError> {
        // Query must be a valid URL for WebRetriever
        if !query.starts_with("http://") && !query.starts_with("https://") {
            return Err(RetrieveError::Exec("only http and https schemes allowed".into()));
        }

        // We use wget inside alpine since it's built-in
        let cmd = ["wget".to_string(), "-qO-".to_string(), query.to_string()];
        
        // For now, since Retriever is async, we can wrap the sync blocking call in spawn_blocking.
        let config_clone = SandboxConfig {
            image: self.config.image.clone(),
            enforce_runsc: self.config.enforce_runsc,
            proxy_url: self.config.proxy_url.clone(),
        };
        let query_clone = query.to_string();

        let content = tokio::task::spawn_blocking(move || {
            let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
            run_in_sandbox(&config_clone, &cmd_refs)
        })
        .await
        .map_err(|e| RetrieveError::Exec(format!("join error: {e}")))?
        .map_err(|e| match e {
            SandboxError::Exec(s) => RetrieveError::Exec(s),
            SandboxError::NotAvailable => RetrieveError::Exec("docker not available".into()),
        })?;

        Ok(vec![RetrievedChunk {
            source: query_clone,
            content,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retrieve_valid_url_downloads_content() {
        let retriever = WebRetriever::new();
        if !retriever.available() {
            return;
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(retriever.retrieve("http://example.com"));
        assert!(result.is_ok(), "failed to retrieve: {:?}", result.err());
        let chunks = result.unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].content.contains("Example Domain"));
    }

    #[test]
    fn retrieve_invalid_scheme_rejected() {
        let retriever = WebRetriever::new();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(retriever.retrieve("ftp://example.com"));
        assert!(result.is_err());
    }

    #[test]
    fn retrieve_non_existent_domain_fails() {
        let retriever = WebRetriever::new();
        if !retriever.available() {
            return;
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(retriever.retrieve("http://this-does-not-exist.example.invalid"));
        assert!(result.is_err());
    }
}
