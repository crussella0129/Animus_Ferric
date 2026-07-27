//! The Web Retriever plane.
use crate::retriever::{RetrieveError, RetrievedChunk, Retriever};
use crate::sandbox::{NetworkPolicy, SandboxConfig, SandboxError, check_available, run_in_sandbox};
use async_trait::async_trait;

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
        // One definition of the default sandbox shape. This used to duplicate
        // `SandboxConfig::default()`'s literal, which meant the airlock defaults
        // could drift apart silently.
        Self {
            config: SandboxConfig::default(),
        }
    }

    pub fn with_runsc(mut self, enforce: bool) -> Self {
        self.config.enforce_runsc = enforce;
        self
    }

    /// Route egress through an allowlist gateway on an **isolated** docker
    /// network (ADR-082). Both arguments are required because both are load
    /// bearing: the network is what *enforces* the airlock, and the URL only
    /// tells cooperative clients where the gateway is.
    ///
    /// Without this the retriever has **no network at all** (ADR-074), which is
    /// deliberate — a web retriever that silently gets unrestricted egress by
    /// default is the failure that guards against. To opt out entirely, see
    /// [`Self::with_network`].
    pub fn with_airlock(
        mut self,
        network: impl Into<String>,
        proxy_url: impl Into<String>,
    ) -> Self {
        self.config.network = NetworkPolicy::Airlock {
            network: network.into(),
            proxy_url: proxy_url.into(),
        };
        self
    }

    /// Set the network policy directly, including
    /// [`NetworkPolicy::Unrestricted`].
    pub fn with_network(mut self, network: NetworkPolicy) -> Self {
        self.config.network = network;
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
            return Err(RetrieveError::Exec(
                "only http and https schemes allowed".into(),
            ));
        }

        // We use wget inside alpine since it's built-in
        let cmd = ["wget".to_string(), "-qO-".to_string(), query.to_string()];

        // For now, since Retriever is async, we can wrap the sync blocking call in spawn_blocking.
        let config_clone = self.config.clone();
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

    /// A retriever configured for a live fetch. Since ADR-074 the default has
    /// **no network and requires gVisor**, so a test that actually reaches the
    /// internet has to opt out of both — which is the point: reaching the
    /// network is now something you can see in the code.
    fn live_fetch_retriever() -> WebRetriever {
        WebRetriever::new()
            .with_runsc(false)
            .with_network(NetworkPolicy::Unrestricted)
    }

    #[test]
    #[cfg_attr(windows, ignore)]
    fn retrieve_valid_url_downloads_content() {
        let retriever = live_fetch_retriever();
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
    #[cfg_attr(windows, ignore)]
    fn retrieve_non_existent_domain_fails() {
        let retriever = live_fetch_retriever();
        if !retriever.available() {
            return;
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(retriever.retrieve("http://this-does-not-exist.example.invalid"));
        assert!(result.is_err());
    }
}
