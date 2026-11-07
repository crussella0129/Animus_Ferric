use clap::Args;
use std::fmt;
use std::future::Future;
use std::path::Path;

use ferric_provider::{MockProvider, Provider};

use crate::server::{
    ManagedDiscoveryScope, ManagedServer, ManagedServerDiscovery, ManagedServerState,
};

/// Connection options for the sole backend — the OpenAI-compatible HTTP valve
/// (llama.cpp / Ollama), which enforces a `response_format` constraint
/// server-side → `ConstrainedJson`. The in-process mistral.rs path (ADR-027)
/// and the `--backend` selector it needed were removed once the valve became
/// the only backend; a second variant re-enters here trivially if one lands.
#[derive(Args, Clone, Default)]
pub struct BackendOpts {
    /// The model string identifier (required for openai backend)
    #[arg(long)]
    pub model: Option<String>,

    /// Explicit OpenAI-compatible API base URL. Without this flag, Ferric uses
    /// one Ready managed local/global/origin registration, defaults only when
    /// the full inventory is Empty, and refuses degraded or ambiguous state.
    #[arg(long)]
    pub api_base: Option<String>,

    /// The API key for the OpenAI-compatible API (for openai backend)
    #[arg(long)]
    pub api_key: Option<String>,
}

const DEFAULT_API_BASE: &str = "http://localhost:1234/v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EndpointSelection {
    Explicit {
        base_url: String,
    },
    Managed {
        scope: ManagedDiscoveryScope,
        server: Box<ManagedServer>,
        explicit_base_url: Option<String>,
    },
    Default {
        base_url: String,
    },
}

impl EndpointSelection {
    pub(crate) fn base_url(&self) -> &str {
        match self {
            Self::Explicit { base_url } | Self::Default { base_url } => base_url,
            Self::Managed { server, .. } => &server.runfile.base_url,
        }
    }

    pub(crate) fn managed(&self) -> Option<(&ManagedDiscoveryScope, &ManagedServer)> {
        match self {
            Self::Managed { scope, server, .. } => Some((scope, server)),
            Self::Explicit { .. } | Self::Default { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EndpointSelectionError {
    Setup(String),
    Degraded(Box<ManagedServerDiscovery>),
    StaleOnly(Box<ManagedServerDiscovery>),
    Conflict(Box<ManagedServerDiscovery>),
    Unverifiable(Box<ManagedServerDiscovery>),
    ExplicitManagedMismatch { explicit: String, managed: String },
}

impl fmt::Display for EndpointSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let issues = |discovery: &ManagedServerDiscovery| match &discovery.state {
            ManagedServerState::StaleOnly { stale } => stale
                .iter()
                .map(|coordinate| coordinate.path.display().to_string())
                .collect::<Vec<_>>()
                .join("; "),
            state => state.issue_summary(),
        };
        match self {
            Self::Setup(detail) => formatter.write_str(detail),
            Self::Degraded(discovery) => write!(
                formatter,
                "managed server discovery is degraded and cannot select a backend: {}",
                issues(discovery)
            ),
            Self::StaleOnly(discovery) => write!(
                formatter,
                "only stale server registrations remain: {}",
                issues(discovery)
            ),
            Self::Conflict(discovery) => write!(
                formatter,
                "managed server registrations conflict: {}",
                issues(discovery)
            ),
            Self::Unverifiable(discovery) => write!(
                formatter,
                "managed server registrations are unverifiable: {}",
                issues(discovery)
            ),
            Self::ExplicitManagedMismatch { explicit, managed } => write!(
                formatter,
                "explicit endpoint {explicit} does not match the ready managed endpoint {managed}"
            ),
        }
    }
}

pub(crate) fn automatic_endpoint_from_discovery(
    scope: ManagedDiscoveryScope,
    discovery: ManagedServerDiscovery,
) -> Result<EndpointSelection, EndpointSelectionError> {
    match &discovery.state {
        ManagedServerState::Empty => Ok(EndpointSelection::Default {
            base_url: DEFAULT_API_BASE.to_string(),
        }),
        ManagedServerState::Ready(server) => Ok(EndpointSelection::Managed {
            scope,
            server: Box::new(server.clone()),
            explicit_base_url: None,
        }),
        ManagedServerState::Degraded { .. } => {
            Err(EndpointSelectionError::Degraded(Box::new(discovery)))
        }
        ManagedServerState::StaleOnly { .. } => {
            Err(EndpointSelectionError::StaleOnly(Box::new(discovery)))
        }
        ManagedServerState::Conflict { .. } => {
            Err(EndpointSelectionError::Conflict(Box::new(discovery)))
        }
        ManagedServerState::Unverifiable { .. } => {
            Err(EndpointSelectionError::Unverifiable(Box::new(discovery)))
        }
    }
}

pub(crate) fn select_endpoint_with<F>(
    explicit: Option<&str>,
    discover: F,
) -> Result<EndpointSelection, EndpointSelectionError>
where
    F: FnOnce() -> Result<(ManagedDiscoveryScope, ManagedServerDiscovery), String>,
{
    if let Some(base_url) = explicit {
        return Ok(EndpointSelection::Explicit {
            base_url: base_url.to_string(),
        });
    }
    let (scope, discovery) = discover().map_err(EndpointSelectionError::Setup)?;
    automatic_endpoint_from_discovery(scope, discovery)
}

pub(crate) fn require_managed_endpoint(
    scope: ManagedDiscoveryScope,
    discovery: ManagedServerDiscovery,
    explicit: Option<&str>,
) -> Result<EndpointSelection, EndpointSelectionError> {
    let mut selection = automatic_endpoint_from_discovery(scope, discovery)?;
    let EndpointSelection::Managed {
        server,
        explicit_base_url,
        ..
    } = &mut selection
    else {
        return Err(EndpointSelectionError::Setup(
            "strict managed mode requires a ready `ferric server`; built-in default selection is not permitted"
                .to_string(),
        ));
    };
    if let Some(explicit) = explicit {
        if normalized_endpoint(explicit) != normalized_endpoint(&server.runfile.base_url) {
            return Err(EndpointSelectionError::ExplicitManagedMismatch {
                explicit: explicit.to_string(),
                managed: server.runfile.base_url.clone(),
            });
        }
        *explicit_base_url = Some(explicit.to_string());
    }
    Ok(selection)
}

fn normalized_endpoint(value: &str) -> &str {
    value.trim_end_matches('/')
}

/// Resolve the endpoint exactly once for commands that launch multiple query
/// processes. Freezing the discovered runfile URL prevents a long benchmark
/// from silently switching servers mid-run.
pub(crate) fn resolved_endpoint(
    explicit: Option<&str>,
) -> Result<EndpointSelection, EndpointSelectionError> {
    let workspace = std::env::current_dir().map_err(|error| {
        EndpointSelectionError::Setup(format!(
            "resolve current directory for server discovery: {error}"
        ))
    })?;
    resolved_endpoint_in(explicit, &workspace)
}

pub(crate) fn resolved_endpoint_in(
    explicit: Option<&str>,
    workspace: &Path,
) -> Result<EndpointSelection, EndpointSelectionError> {
    select_endpoint_with(explicit, || {
        let scope = ManagedDiscoveryScope::for_workspace(workspace)?;
        let discovery = crate::server::discover_managed_server_in(&scope);
        Ok((scope, discovery))
    })
}

#[cfg(feature = "backend-openai")]
pub async fn create_provider(
    opts: &BackendOpts,
) -> Result<Box<dyn Provider + Send + Sync>, String> {
    let workspace = std::env::current_dir()
        .map_err(|error| format!("resolve current directory for server discovery: {error}"))?;
    create_provider_in(opts, &workspace).await
}

#[cfg(feature = "backend-openai")]
pub(crate) async fn create_provider_in(
    opts: &BackendOpts,
    workspace: &Path,
) -> Result<Box<dyn Provider + Send + Sync>, String> {
    use ferric_provider::openai::{OpenAiConfig, OpenAiProvider};
    let model_id = opts.model.clone().unwrap_or_else(|| "default".to_string());
    let api_key = opts
        .api_key
        .clone()
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());
    let base_url = resolved_endpoint_in(opts.api_base.as_deref(), workspace)
        .map_err(|error| error.to_string())?
        .base_url()
        .to_string();
    let config = OpenAiConfig {
        base_url,
        api_key: api_key.unwrap_or_else(|| "ollama".to_string()),
        model: model_id,
    };
    Ok(Box::new(OpenAiProvider::new(config)))
}

/// What a caller is told when the binary has no backend compiled in. One
/// definition: this text lived in four byte-identical copies, each
/// unreachable in a normally-built binary and so each free to drift
/// unnoticed. `ferric dream` deliberately keeps its own wording — it has no
/// `--mock`, so pointing at one would be wrong.
#[cfg_attr(feature = "backend-openai", allow(dead_code))]
pub(crate) const BACKEND_FEATURE_MISSING: &str = "this binary was built without backend features; \
     rebuild with `cargo build --features backend-openai`, or use --mock";

/// A backend a command has finished resolving: either the scripted mock, or a
/// real provider owning the ONE runtime it will be driven on.
pub(crate) enum ResolvedBackend {
    Mock,
    // Constructed only where a backend is compiled in (and by tests).
    #[cfg_attr(not(feature = "backend-openai"), allow(dead_code))]
    Real {
        provider: Box<dyn Provider + Send + Sync>,
        runtime: tokio::runtime::Runtime,
    },
}

impl ResolvedBackend {
    /// The backend a run asked for: the mock, or a real provider bound to the
    /// same selected workspace as config — or why this binary cannot have one.
    pub(crate) fn resolve(
        mock: bool,
        opts: &BackendOpts,
        workspace: &Path,
    ) -> Result<Self, String> {
        if mock {
            return Ok(Self::Mock);
        }
        #[cfg(feature = "backend-openai")]
        {
            let runtime =
                tokio::runtime::Runtime::new().map_err(|e| format!("tokio runtime: {e}"))?;
            let provider = runtime.block_on(create_provider_in(opts, workspace))?;
            Ok(Self::Real { provider, runtime })
        }
        #[cfg(not(feature = "backend-openai"))]
        {
            let _ = (opts, workspace);
            Err(BACKEND_FEATURE_MISSING.to_string())
        }
    }

    /// Drive `future` to completion: the mock has no ambient runtime by
    /// design (ADR-010 keeps the loop executor-agnostic), a real provider runs
    /// on its own.
    pub(crate) fn block_on<F: Future>(&self, future: F) -> F::Output {
        match self {
            Self::Mock => futures_executor::block_on(future),
            Self::Real { runtime, .. } => runtime.block_on(future),
        }
    }

    /// Call `f` with this backend's provider. A real provider is shared for
    /// the backend's lifetime; the scripted mock is a finite script, so every
    /// call gets a fresh one from `mock`.
    pub(crate) fn with_provider<T>(
        &self,
        mock: impl FnOnce() -> MockProvider,
        f: impl FnOnce(&dyn Provider) -> T,
    ) -> T {
        match self {
            Self::Mock => f(&mock()),
            Self::Real { provider, .. } => f(provider.as_ref()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one place this string can be checked: it is a user-facing error in
    /// a path a normally-built binary cannot reach, and it went three sprints
    /// as three untested copies. It has to name the feature to rebuild with
    /// *and* the way to proceed without one — a diagnostic that says only
    /// "unsupported" leaves the user nowhere.
    #[test]
    fn the_backend_feature_diagnostic_names_the_feature_and_the_alternative() {
        assert!(BACKEND_FEATURE_MISSING.contains("backend-openai"));
        assert!(BACKEND_FEATURE_MISSING.contains("--mock"));
    }

    #[test]
    fn api_base_precedence() {
        let explicit = select_endpoint_with(Some("http://explicit/v1"), || {
            panic!("explicit endpoint selection must not inspect managed registrations")
        })
        .unwrap();
        assert!(matches!(explicit, EndpointSelection::Explicit { .. }));
        assert_eq!(explicit.base_url(), "http://explicit/v1");
    }

    #[test]
    fn selected_workspace_drives_real_provider() {
        let invocation = tempfile::tempdir().unwrap();
        let selected = tempfile::tempdir().unwrap();
        std::fs::create_dir(selected.path().join(".ferric")).unwrap();
        std::fs::write(
            selected.path().join(".ferric/server.json"),
            "invalid registration",
        )
        .unwrap();
        // Exercise the real registration reader with explicit isolated roots;
        // no global chdir, environment changes, or live network probes.
        let discover = |workspace: &Path| {
            let scope = ManagedDiscoveryScope {
                workspace: workspace.to_path_buf(),
                global: None,
            };
            let discovery = crate::server::discover_managed_server_in(&scope);
            Ok((scope, discovery))
        };
        assert!(matches!(
            select_endpoint_with(None, || discover(invocation.path())),
            Ok(EndpointSelection::Default { .. })
        ));
        assert!(
            select_endpoint_with(None, || discover(selected.path())).is_err(),
            "the selected blocked registration must not become the invocation default"
        );
        assert!(selected.path().join(".ferric/server.json").exists());
    }

    #[test]
    fn explicit_endpoint_and_ambiguous_discovery_matrix() {
        let dir = tempfile::tempdir().unwrap();
        let explicit = select_endpoint_with(Some("http://explicit/v1"), || {
            panic!("explicit endpoint must not discover another workspace or server")
        })
        .unwrap();
        assert_eq!(explicit.base_url(), "http://explicit/v1");
        for state in [
            ManagedServerState::Conflict { issues: Vec::new() },
            ManagedServerState::Unverifiable { issues: Vec::new() },
            ManagedServerState::StaleOnly { stale: Vec::new() },
        ] {
            let scope = ManagedDiscoveryScope {
                workspace: dir.path().to_path_buf(),
                global: None,
            };
            let mut discovery = crate::server::discover_managed_server_in(&scope);
            discovery.state = state;
            assert!(select_endpoint_with(None, || Ok((scope, discovery))).is_err());
        }
    }
}
