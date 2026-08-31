//! Closed, bounded adapter for the one Tailscale Serve coordinate Ferric owns.
//!
//! This module deliberately has no generic command entry point. It reads only
//! LocalAPI status and Serve config, binds each snapshot with a same-session
//! identity sandwich, and mutates through one exact ETag/If-Match CAS. The
//! only authored handler is a high-entropy `/_ferric/<token>` path; there is no
//! route to reset, a root handler, or an unscoped teardown.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

use crate::tailscale_localapi::{
    LocalApiError, LocalApiSession, LocalApiStatus, ServeConfigCasError, TailscaleLocalApiClient,
};

pub(crate) const OWNERSHIP_VERSION: u8 = 2;
pub(crate) const HTTPS_PORT: u16 = 443;
const TOKEN_BYTES: usize = 16;
const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;
const STATUS_SHA256_HEX_LEN: usize = 64;

/// Durable, additive ownership evidence carried by a schema-v2 server record.
///
/// `before_status_sha256` is provenance, not teardown authority. Destructive
/// authority always comes from a fresh exact-coordinate comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TailscaleServeOwnership {
    pub version: u8,
    pub token: String,
    /// Opaque stable node identity from the same tailscaled instance that
    /// owns the Serve config. This binds the profile/node; the separately
    /// journaled FQDN detects a rename for publication while still permitting
    /// cleanup of that node's old-name coordinate.
    pub stable_node_id: String,
    pub fqdn: String,
    pub https_port: u16,
    pub mount_path: String,
    pub proxy_target: String,
    pub remote_base_url: String,
    pub before_status_sha256: String,
    /// Whether each parent container existed in the exact pre-apply Serve
    /// snapshot. Teardown may remove only scaffolding Ferric introduced; an
    /// operator-owned empty map or host shell is still state worth preserving.
    pub tcp_map_preexisting: bool,
    pub tcp_https_preexisting: bool,
    pub web_map_preexisting: bool,
    pub web_host_preexisting: bool,
    /// `false` is the durable write-ahead/apply-ambiguous phase. An absent
    /// path cannot authorize journal deletion in that phase because a
    /// canceled LocalAPI request may still commit later. Ferric promotes both
    /// journals to `true` only after the exact proxy has been observed.
    #[serde(default)]
    pub apply_confirmed: bool,
}

impl TailscaleServeOwnership {
    pub(crate) fn same_coordinate(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.apply_confirmed = false;
        right.apply_confirmed = false;
        left == right
    }

    /// The pre-apply refresh is the one authorized transition that replaces
    /// snapshot provenance before any external mutation. This comparison must
    /// not be used to reconcile mirrors or authorize teardown.
    pub(crate) fn same_endpoint_coordinate(&self, other: &Self) -> bool {
        let mut left = self.clone();
        let mut right = other.clone();
        left.apply_confirmed = false;
        right.apply_confirmed = false;
        left.before_status_sha256.clear();
        right.before_status_sha256.clear();
        left.tcp_map_preexisting = false;
        right.tcp_map_preexisting = false;
        left.tcp_https_preexisting = false;
        right.tcp_https_preexisting = false;
        left.web_map_preexisting = false;
        right.web_map_preexisting = false;
        left.web_host_preexisting = false;
        right.web_host_preexisting = false;
        left == right
    }

    pub(crate) fn refreshed_for_preapply(
        &self,
        observation: &ServeStatusObservation,
    ) -> Result<Self, TailscaleServeError> {
        self.validate()?;
        if observation.fqdn != self.fqdn
            || observation.https_port != self.https_port
            || observation.mount_path != self.mount_path
        {
            return Err(TailscaleServeError::InvalidStatus(
                "authoritative pre-apply observation does not describe the journaled coordinate"
                    .to_string(),
            ));
        }
        let observed_identity = observation.identity.as_ref().ok_or_else(|| {
            TailscaleServeError::InvalidIdentity(
                "pre-apply Serve observation is not bound to a LocalAPI identity sandwich"
                    .to_string(),
            )
        })?;
        observed_identity.require_same_publication_identity(self)?;
        if observation.path_state != ServePathState::Absent {
            return Err(TailscaleServeError::InvalidStatus(
                "authoritative pre-apply Serve path is not absent".to_string(),
            ));
        }
        if let Some(hazard) = observation.preapply_hazard() {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "{hazard}; authoritative pre-apply refresh was refused"
            )));
        }
        let mut refreshed = self.clone();
        refreshed.before_status_sha256 = observation.status_sha256.clone();
        refreshed.tcp_map_preexisting = observation.scaffold.tcp_map_present;
        refreshed.tcp_https_preexisting = observation.scaffold.tcp_https_present;
        refreshed.web_map_preexisting = observation.scaffold.web_map_present;
        refreshed.web_host_preexisting = observation.scaffold.web_host_present;
        refreshed.apply_confirmed = false;
        refreshed.validate()?;
        Ok(refreshed)
    }

    pub(crate) fn validate(&self) -> Result<(), TailscaleServeError> {
        if self.version != OWNERSHIP_VERSION {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "unsupported Tailscale Serve ownership version {}",
                self.version
            )));
        }
        validate_token(&self.token)?;
        validate_stable_node_id(&self.stable_node_id)?;
        validate_fqdn(&self.fqdn)?;
        if self.https_port != HTTPS_PORT {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve ownership must use HTTPS port {HTTPS_PORT}"
            )));
        }
        let expected_mount = mount_path_for_token(&self.token)?;
        if self.mount_path != expected_mount {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve mount_path must be {expected_mount}"
            )));
        }
        let port = proxy_target_port(&self.proxy_target)?;
        let expected_target = proxy_target_for_port(port);
        if self.proxy_target != expected_target {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve proxy_target must be {expected_target}"
            )));
        }
        let expected_remote = remote_base_for(&self.fqdn, &self.mount_path);
        if self.remote_base_url != expected_remote {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve remote_base_url must be {expected_remote}"
            )));
        }
        validate_lower_hex(
            "before_status_sha256",
            &self.before_status_sha256,
            STATUS_SHA256_HEX_LEN,
        )?;
        if self.tcp_https_preexisting && !self.tcp_map_preexisting {
            return Err(TailscaleServeError::InvalidOwnership(
                "pre-existing TCP port 443 requires a pre-existing TCP map".to_string(),
            ));
        }
        if self.web_host_preexisting && !self.web_map_preexisting {
            return Err(TailscaleServeError::InvalidOwnership(
                "pre-existing Web host requires a pre-existing Web map".to_string(),
            ));
        }
        if self.web_host_preexisting && !self.tcp_https_preexisting {
            return Err(TailscaleServeError::InvalidOwnership(
                "pre-existing HTTPS Web host requires pre-existing TCP HTTPS state".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_for_port(&self, port: u16) -> Result<(), TailscaleServeError> {
        self.validate()?;
        if port == 0 || self.proxy_target != proxy_target_for_port(port) {
            return Err(TailscaleServeError::InvalidOwnership(format!(
                "Tailscale Serve proxy_target does not match registered loopback port {port}"
            )));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn proxy_port(&self) -> Result<u16, TailscaleServeError> {
        proxy_target_port(&self.proxy_target)
    }
}

/// High-entropy coordinate prepared before any engine or Serve side effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TailscaleServeCoordinate {
    pub token: String,
    pub stable_node_id: String,
    pub fqdn: String,
    pub https_port: u16,
    pub mount_path: String,
    pub proxy_target: String,
    pub remote_base_url: String,
}

impl TailscaleServeCoordinate {
    pub(crate) fn into_ownership(
        self,
        observation: &ServeStatusObservation,
    ) -> Result<TailscaleServeOwnership, TailscaleServeError> {
        if observation.fqdn != self.fqdn
            || observation.https_port != self.https_port
            || observation.mount_path != self.mount_path
        {
            return Err(TailscaleServeError::InvalidStatus(
                "pre-apply Serve observation does not describe the prepared coordinate".to_string(),
            ));
        }
        let observed_identity = observation.identity.as_ref().ok_or_else(|| {
            TailscaleServeError::InvalidIdentity(
                "pre-apply Serve observation is not bound to a LocalAPI identity sandwich"
                    .to_string(),
            )
        })?;
        if observed_identity.stable_node_id != self.stable_node_id
            || observed_identity.fqdn != self.fqdn
        {
            return Err(TailscaleServeError::InvalidIdentity(
                "pre-apply Serve observation identity differs from the prepared coordinate"
                    .to_string(),
            ));
        }
        observed_identity.require_publication_ready()?;
        if observation.path_state != ServePathState::Absent {
            return Err(TailscaleServeError::InvalidStatus(
                "pre-apply Serve observation is not absent".to_string(),
            ));
        }
        if let Some(hazard) = observation.preapply_hazard() {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "{hazard}; launch preflight was refused"
            )));
        }
        let ownership = TailscaleServeOwnership {
            version: OWNERSHIP_VERSION,
            token: self.token,
            stable_node_id: self.stable_node_id,
            fqdn: self.fqdn,
            https_port: self.https_port,
            mount_path: self.mount_path,
            proxy_target: self.proxy_target,
            remote_base_url: self.remote_base_url,
            before_status_sha256: observation.status_sha256.clone(),
            tcp_map_preexisting: observation.scaffold.tcp_map_present,
            tcp_https_preexisting: observation.scaffold.tcp_https_present,
            web_map_preexisting: observation.scaffold.web_map_present,
            web_host_preexisting: observation.scaffold.web_host_present,
            apply_confirmed: false,
        };
        ownership.validate()?;
        Ok(ownership)
    }
}

pub(crate) trait EntropySource {
    fn fill_128(&self, destination: &mut [u8; TOKEN_BYTES]) -> Result<(), TailscaleServeError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OsEntropy;

impl EntropySource for OsEntropy {
    fn fill_128(&self, destination: &mut [u8; TOKEN_BYTES]) -> Result<(), TailscaleServeError> {
        // Exactly one 16-byte OS-CSPRNG fill: no truncation, expansion, or
        // pseudo-random fallback can silently weaken the ownership coordinate.
        getrandom::fill(destination).map_err(|error| {
            TailscaleServeError::Entropy(format!(
                "could not obtain 128 bits from the operating-system CSPRNG: {error}"
            ))
        })
    }
}

#[cfg(test)]
pub(crate) fn prepare_coordinate_with_entropy<E: EntropySource>(
    port: u16,
    identity: &TailscaleIdentity,
    entropy: &E,
) -> Result<TailscaleServeCoordinate, TailscaleServeError> {
    let token = generate_token_with_entropy(entropy)?;
    coordinate_from_token(port, identity, token)
}

/// Draw the ownership coordinate before identity or command probes. This
/// split lets launch preflight prove that entropy failure advances neither an
/// engine counter nor a Tailscale command counter.
pub(crate) fn generate_token() -> Result<String, TailscaleServeError> {
    generate_token_with_entropy(&OsEntropy)
}

pub(crate) fn generate_token_with_entropy<E: EntropySource>(
    entropy: &E,
) -> Result<String, TailscaleServeError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    entropy.fill_128(&mut bytes)?;
    Ok(hex::encode(bytes))
}

pub(crate) fn coordinate_from_token(
    port: u16,
    identity: &TailscaleIdentity,
    token: String,
) -> Result<TailscaleServeCoordinate, TailscaleServeError> {
    if port == 0 {
        return Err(TailscaleServeError::InvalidOwnership(
            "Tailscale Serve requires a nonzero loopback target port".to_string(),
        ));
    }
    identity.require_publication_ready()?;
    let mount_path = mount_path_for_token(&token)?;
    let proxy_target = proxy_target_for_port(port);
    let remote_base_url = remote_base_for(&identity.fqdn, &mount_path);
    Ok(TailscaleServeCoordinate {
        token,
        stable_node_id: identity.stable_node_id.clone(),
        fqdn: identity.fqdn.clone(),
        https_port: HTTPS_PORT,
        mount_path,
        proxy_target,
        remote_base_url,
    })
}

/// Identity and HTTPS-publication authority read from one bounded LocalAPI
/// status response. `stable_node_id` and `fqdn` are journal coordinates;
/// capability and certificate fields are current preconditions only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TailscaleIdentity {
    pub stable_node_id: String,
    pub fqdn: String,
    pub backend_running: bool,
    pub https_capable: bool,
    pub certificate_domain: bool,
}

impl TailscaleIdentity {
    pub(crate) fn require_publication_ready(&self) -> Result<(), TailscaleServeError> {
        validate_stable_node_id(&self.stable_node_id)?;
        validate_fqdn(&self.fqdn)?;
        if !self.backend_running {
            return Err(TailscaleServeError::InvalidIdentity(
                "tailscaled is not in BackendState Running".to_string(),
            ));
        }
        if !self.https_capable || !self.certificate_domain {
            return Err(TailscaleServeError::InvalidIdentity(format!(
                "HTTPS certificate publication is not enabled for {}; enable Tailscale HTTPS before retrying",
                self.fqdn
            )));
        }
        Ok(())
    }

    pub(crate) fn require_same_publication_identity(
        &self,
        ownership: &TailscaleServeOwnership,
    ) -> Result<(), TailscaleServeError> {
        self.require_publication_ready()?;
        if self.stable_node_id != ownership.stable_node_id || self.fqdn != ownership.fqdn {
            return Err(TailscaleServeError::InvalidIdentity(format!(
                "current node identity ({}, {}) differs from journaled Serve identity ({}, {})",
                self.stable_node_id, self.fqdn, ownership.stable_node_id, ownership.fqdn
            )));
        }
        Ok(())
    }

    /// Cleanup remains authorized across a node rename or later HTTPS-policy
    /// change, but never across a profile/node switch. The mutation still
    /// targets only the journaled old FQDN and exact proxy handler.
    pub(crate) fn require_cleanup_identity(
        &self,
        ownership: &TailscaleServeOwnership,
    ) -> Result<(), TailscaleServeError> {
        validate_stable_node_id(&self.stable_node_id)?;
        validate_fqdn(&self.fqdn)?;
        if self.stable_node_id != ownership.stable_node_id {
            return Err(TailscaleServeError::InvalidIdentity(format!(
                "current stable node ID {} differs from journaled Serve node {}",
                self.stable_node_id, ownership.stable_node_id
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ServePathState {
    Absent,
    Proxy { target: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServeStatusObservation {
    pub fqdn: String,
    pub https_port: u16,
    pub mount_path: String,
    pub status_sha256: String,
    pub path_state: ServePathState,
    /// Identity sandwich from the same LocalAPI session as the Serve snapshot.
    /// Pure JSON projections intentionally leave this unset until an adapter
    /// proves status-before/config/status-after equality.
    pub identity: Option<TailscaleIdentity>,
    pub scaffold: ServeScaffoldState,
    /// The top-level TCP 443 entry is the pinned HTTPS-only shape Ferric can
    /// safely share for publication.
    pub https_mode_compatible: bool,
    /// A true AllowFunnel bit on Ferric's host would widen the endpoint beyond
    /// the intended tailnet-only surface.
    pub funnel_enabled: bool,
    /// A foreground TCP 443 or exact-host entry wins Tailscale's effective
    /// lookup over the stored top-level coordinate. Scoped cleanup may still
    /// use `path_state`; readiness must remain fail-closed while shadowed.
    pub foreground_shadows: bool,
    /// An expected-host child or trailing-slash alias beneath the owned mount
    /// wins Tailscale's longest-path lookup. Cleanup may remove Ferric's exact
    /// parent while preserving this operator/hostile route, but must retain
    /// journals and report the residual until it is resolved manually.
    pub route_shadow: Option<String>,
    /// True only when the daemon's routing schema is the exact pinned version.
    /// A future-version cleanup may remove the known handler conservatively,
    /// but unknown routing fields prevent it from proving global absence.
    pub cleanup_semantics_pinned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ServeScaffoldState {
    pub tcp_map_present: bool,
    pub tcp_https_present: bool,
    pub web_map_present: bool,
    pub web_host_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnedServeState {
    Absent,
    Exact,
    Replaced { observed_target: String },
}

impl ServeStatusObservation {
    pub(crate) fn require_publication_identity(
        &self,
        ownership: &TailscaleServeOwnership,
    ) -> Result<(), TailscaleServeError> {
        self.identity
            .as_ref()
            .ok_or_else(|| {
                TailscaleServeError::InvalidIdentity(
                    "Serve observation lacks a LocalAPI identity sandwich".to_string(),
                )
            })?
            .require_same_publication_identity(ownership)
    }

    pub(crate) fn require_cleanup_identity(
        &self,
        ownership: &TailscaleServeOwnership,
    ) -> Result<(), TailscaleServeError> {
        self.identity
            .as_ref()
            .ok_or_else(|| {
                TailscaleServeError::InvalidIdentity(
                    "Serve cleanup observation lacks a LocalAPI identity sandwich".to_string(),
                )
            })?
            .require_cleanup_identity(ownership)
    }

    pub(crate) fn publication_hazard(&self) -> Option<String> {
        if let Some(path) = &self.route_shadow {
            return Some(format!(
                "Web handler {path} overrides owned path {}",
                self.mount_path
            ));
        }
        if !self.https_mode_compatible {
            return Some(format!(
                "TCP port {} is not in the compatible HTTPS-only mode",
                self.https_port
            ));
        }
        if self.funnel_enabled {
            return Some(format!(
                "AllowFunnel is enabled for {}:{}",
                self.fqdn, self.https_port
            ));
        }
        if self.foreground_shadows {
            return Some(format!(
                "foreground Serve state shadows {}:{}",
                self.fqdn, self.https_port
            ));
        }
        None
    }

    pub(crate) fn preapply_hazard(&self) -> Option<String> {
        if let Some(path) = &self.route_shadow {
            return Some(format!(
                "Web handler {path} overrides owned path {}",
                self.mount_path
            ));
        }
        if self.scaffold.tcp_https_present && !self.https_mode_compatible {
            return Some(format!(
                "pre-existing TCP port {} is not in compatible HTTPS-only mode",
                self.https_port
            ));
        }
        if self.funnel_enabled {
            return Some(format!(
                "AllowFunnel is enabled for {}:{}",
                self.fqdn, self.https_port
            ));
        }
        if self.foreground_shadows {
            return Some(format!(
                "foreground Serve state shadows {}:{}",
                self.fqdn, self.https_port
            ));
        }
        None
    }

    pub(crate) fn owned_state(
        &self,
        ownership: &TailscaleServeOwnership,
    ) -> Result<OwnedServeState, TailscaleServeError> {
        ownership.validate()?;
        if self.fqdn != ownership.fqdn
            || self.https_port != ownership.https_port
            || self.mount_path != ownership.mount_path
        {
            return Err(TailscaleServeError::InvalidStatus(
                "Serve observation does not describe the ownership coordinate".to_string(),
            ));
        }
        Ok(match &self.path_state {
            ServePathState::Absent => OwnedServeState::Absent,
            ServePathState::Proxy { target } if target == &ownership.proxy_target => {
                OwnedServeState::Exact
            }
            ServePathState::Proxy { target } => OwnedServeState::Replaced {
                observed_target: target.clone(),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TailscaleServeError {
    Entropy(String),
    LocalApiNoMutation(String),
    LocalApiIndeterminate(String),
    InvalidIdentity(String),
    InvalidStatus(String),
    InvalidOwnership(String),
}

impl fmt::Display for TailscaleServeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entropy(detail) => write!(formatter, "{detail}"),
            Self::LocalApiNoMutation(detail) => write!(formatter, "{detail}"),
            Self::LocalApiIndeterminate(detail) => write!(formatter, "{detail}"),
            Self::InvalidIdentity(detail) => {
                write!(formatter, "invalid Tailscale identity: {detail}")
            }
            Self::InvalidStatus(detail) => {
                write!(formatter, "invalid Tailscale Serve status: {detail}")
            }
            Self::InvalidOwnership(detail) => {
                write!(formatter, "invalid Tailscale Serve ownership: {detail}")
            }
        }
    }
}

impl std::error::Error for TailscaleServeError {}

impl TailscaleServeError {
    pub(crate) fn may_have_mutated(&self) -> bool {
        matches!(self, Self::LocalApiIndeterminate(_))
    }
}

/// Closed production adapter for the one tailscaled LocalAPI endpoint selected
/// by the platform. There is no generic command or arbitrary HTTP surface.
#[derive(Debug, Clone)]
pub(crate) struct TailscaleServeAdapter {
    client: TailscaleLocalApiClient,
}

impl TailscaleServeAdapter {
    pub(crate) fn native() -> Self {
        Self {
            client: TailscaleLocalApiClient::new(),
        }
    }

    #[cfg(test)]
    fn with_client(client: TailscaleLocalApiClient) -> Self {
        Self { client }
    }

    fn strict_observation(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError> {
        let mut session = self
            .client
            .open_session()
            .map_err(|error| localapi_no_mutation("open the pinned LocalAPI session", error))?;
        let before = session
            .get_status()
            .map_err(|error| localapi_no_mutation("read status before Serve config", error))?;
        let config = session
            .get_serve_config()
            .map_err(|error| localapi_no_mutation("read the Serve config", error))?;
        let after = session
            .get_status()
            .map_err(|error| localapi_no_mutation("read status after Serve config", error))?;
        let identity = require_identity_sandwich(before.raw_json(), after.raw_json(), true)?;
        if identity.fqdn != fqdn {
            return Err(TailscaleServeError::InvalidIdentity(format!(
                "LocalAPI identity {} does not match requested Serve host {fqdn}",
                identity.fqdn
            )));
        }
        let mut observation = project_localapi_status(config.raw_json(), fqdn, mount_path)?;
        observation.identity = Some(identity);
        Ok(observation)
    }

    fn cleanup_session(
        &self,
    ) -> Result<(LocalApiSession, LocalApiStatus, bool), TailscaleServeError> {
        let mut pinned = self
            .client
            .open_session()
            .map_err(|error| localapi_no_mutation("open the cleanup session", error))?;
        match pinned.get_status() {
            Ok(status) => Ok((pinned, status, true)),
            Err(LocalApiError::IncompatibleDaemon(_)) => {
                // Compatibility is established by the first authenticated
                // response, not by opening the transport. Drop the rejected
                // connection and restart the entire identity/config sandwich.
                let mut drift = self
                    .client
                    .reopen_cleanup_session(pinned)
                    .map_err(|error| {
                        localapi_no_mutation("open a bounded version-drift cleanup session", error)
                    })?;
                let status = drift.get_status().map_err(|error| {
                    localapi_no_mutation("read bounded version-drift cleanup status", error)
                })?;
                Ok((drift, status, false))
            }
            Err(error) => Err(localapi_no_mutation("read pinned cleanup status", error)),
        }
    }

    fn cleanup_observation(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError> {
        let (mut session, before, pinned) = self.cleanup_session()?;
        let config = session
            .get_serve_config()
            .map_err(|error| localapi_no_mutation("read cleanup Serve config", error))?;
        let after = session
            .get_status()
            .map_err(|error| localapi_no_mutation("read cleanup status after config", error))?;
        let identity = require_identity_sandwich(before.raw_json(), after.raw_json(), false)?;
        let mut observation = if pinned {
            project_localapi_status(config.raw_json(), fqdn, mount_path)?
        } else {
            project_localapi_status_for_cleanup(config.raw_json(), fqdn, mount_path)?
        };
        observation.identity = Some(identity);
        observation.cleanup_semantics_pinned = pinned;
        Ok(observation)
    }
}

pub(crate) trait TailscaleServeEffects {
    fn self_identity(&self) -> Result<TailscaleIdentity, TailscaleServeError>;
    fn probe_status(&self, fqdn: &str) -> Result<String, TailscaleServeError>;
    fn observe_coordinate(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError>;
    fn observe_coordinate_for_cleanup(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError> {
        self.observe_coordinate(fqdn, mount_path)
    }
    fn apply(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError>;
    fn off(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError>;
}

impl TailscaleServeEffects for TailscaleServeAdapter {
    fn self_identity(&self) -> Result<TailscaleIdentity, TailscaleServeError> {
        let status = self
            .client
            .get_status()
            .map_err(|error| localapi_no_mutation("read LocalAPI identity", error))?;
        parse_localapi_identity(status.raw_json())
    }

    fn probe_status(&self, fqdn: &str) -> Result<String, TailscaleServeError> {
        validate_fqdn(fqdn)?;
        let mut session = self.client.open_session().map_err(|error| {
            localapi_no_mutation("open the pinned LocalAPI status session", error)
        })?;
        let before = session
            .get_status()
            .map_err(|error| localapi_no_mutation("read status before Serve config", error))?;
        let config = session
            .get_serve_config()
            .map_err(|error| localapi_no_mutation("read the Serve config", error))?;
        let after = session
            .get_status()
            .map_err(|error| localapi_no_mutation("read status after Serve config", error))?;
        let identity = require_identity_sandwich(before.raw_json(), after.raw_json(), true)?;
        if identity.fqdn != fqdn {
            return Err(TailscaleServeError::InvalidIdentity(format!(
                "LocalAPI identity {} does not match requested Serve host {fqdn}",
                identity.fqdn
            )));
        }
        Ok(config.etag().to_string())
    }

    fn observe_coordinate(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError> {
        self.strict_observation(fqdn, mount_path)
    }

    fn observe_coordinate_for_cleanup(
        &self,
        fqdn: &str,
        mount_path: &str,
    ) -> Result<ServeStatusObservation, TailscaleServeError> {
        self.cleanup_observation(fqdn, mount_path)
    }

    fn apply(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError> {
        ownership.validate()?;
        let mut session = self.client.open_session().map_err(|error| {
            localapi_no_mutation("open the pinned LocalAPI apply session", error)
        })?;
        let status_a = session
            .get_status()
            .map_err(|error| localapi_no_mutation("read pre-apply status", error))?;
        let config = session
            .get_serve_config()
            .map_err(|error| localapi_no_mutation("read pre-apply Serve config", error))?;
        let status_b = session
            .get_status()
            .map_err(|error| localapi_no_mutation("recheck pre-apply status", error))?;
        let identity = require_identity_sandwich(status_a.raw_json(), status_b.raw_json(), true)?;
        identity.require_same_publication_identity(ownership)?;
        let body = prepare_localapi_apply(config.raw_json(), ownership)?;
        map_cas_result(
            session.compare_and_set_serve_config(config.etag(), &body),
            "apply",
        )?;

        let status_c = session
            .get_status()
            .map_err(|error| localapi_indeterminate("read post-apply status", error.to_string()))?;
        let applied = session.get_serve_config().map_err(|error| {
            localapi_indeterminate("read post-apply Serve config", error.to_string())
        })?;
        let status_d = session.get_status().map_err(|error| {
            localapi_indeterminate("recheck post-apply status", error.to_string())
        })?;
        let post_identity =
            require_identity_sandwich(status_c.raw_json(), status_d.raw_json(), true).map_err(
                |error| localapi_indeterminate("bind post-apply identity", error.to_string()),
            )?;
        post_identity
            .require_same_publication_identity(ownership)
            .map_err(|error| {
                localapi_indeterminate("verify post-apply identity", error.to_string())
            })?;
        let mut observation =
            project_localapi_status(applied.raw_json(), &ownership.fqdn, &ownership.mount_path)
                .map_err(|error| {
                    localapi_indeterminate("project post-apply Serve config", error.to_string())
                })?;
        observation.identity = Some(post_identity);
        observation
            .require_publication_identity(ownership)
            .map_err(|error| {
                localapi_indeterminate("verify post-apply identity", error.to_string())
            })?;
        let post_state = observation.owned_state(ownership).map_err(|error| {
            localapi_indeterminate("project post-apply path", error.to_string())
        })?;
        if post_state != OwnedServeState::Exact {
            return Err(localapi_indeterminate(
                "verify post-apply path",
                "the exact proxy was not present".to_string(),
            ));
        }
        if let Some(hazard) = observation.publication_hazard() {
            return Err(localapi_indeterminate("verify post-apply policy", hazard));
        }
        Ok(())
    }

    fn off(&self, ownership: &TailscaleServeOwnership) -> Result<(), TailscaleServeError> {
        ownership.validate()?;
        let (mut session, status_a, pinned) = self.cleanup_session()?;
        let config = session
            .get_serve_config()
            .map_err(|error| localapi_no_mutation("read pre-cleanup Serve config", error))?;
        let status_b = session
            .get_status()
            .map_err(|error| localapi_no_mutation("recheck pre-cleanup status", error))?;
        let identity = require_identity_sandwich(status_a.raw_json(), status_b.raw_json(), false)?;
        identity.require_cleanup_identity(ownership)?;
        let body = if pinned {
            prepare_localapi_off(config.raw_json(), ownership)?
        } else {
            prepare_localapi_off_preserving_scaffold(config.raw_json(), ownership)?
        };
        let Some(body) = body else {
            let observation = project_localapi_status_for_cleanup(
                config.raw_json(),
                &ownership.fqdn,
                &ownership.mount_path,
            )?;
            if let Some(path) = observation.route_shadow {
                return Err(TailscaleServeError::LocalApiNoMutation(format!(
                    "the exact Ferric Serve proxy is already absent, but Web handler {path} still overrides the journaled mount; ownership journals must remain"
                )));
            }
            if observation.foreground_shadows {
                return Err(TailscaleServeError::LocalApiNoMutation(format!(
                    "the exact Ferric Serve proxy is already absent, but foreground Serve state still shadows {}:{}; ownership journals must remain",
                    ownership.fqdn, ownership.https_port
                )));
            }
            if !pinned {
                return Err(TailscaleServeError::LocalApiNoMutation(
                    "the exact Ferric Serve handler is absent on a newer daemon, but unknown routing semantics prevent proving endpoint absence; ownership journals must remain"
                        .to_string(),
                ));
            }
            return Ok(());
        };
        map_cas_result(
            session.compare_and_set_serve_config(config.etag(), &body),
            "cleanup",
        )?;

        let status_c = session.get_status().map_err(|error| {
            localapi_indeterminate("read post-cleanup status", error.to_string())
        })?;
        let removed = session.get_serve_config().map_err(|error| {
            localapi_indeterminate("read post-cleanup Serve config", error.to_string())
        })?;
        let status_d = session.get_status().map_err(|error| {
            localapi_indeterminate("recheck post-cleanup status", error.to_string())
        })?;
        let post_identity =
            require_identity_sandwich(status_c.raw_json(), status_d.raw_json(), false).map_err(
                |error| localapi_indeterminate("bind post-cleanup identity", error.to_string()),
            )?;
        post_identity
            .require_cleanup_identity(ownership)
            .map_err(|error| {
                localapi_indeterminate("verify post-cleanup identity", error.to_string())
            })?;
        let observation = project_localapi_status_for_cleanup(
            removed.raw_json(),
            &ownership.fqdn,
            &ownership.mount_path,
        )
        .map_err(|error| {
            localapi_indeterminate("project post-cleanup Serve config", error.to_string())
        })?;
        let post_state = observation.owned_state(ownership).map_err(|error| {
            localapi_indeterminate("project post-cleanup path", error.to_string())
        })?;
        if post_state != OwnedServeState::Absent {
            return Err(localapi_indeterminate(
                "verify post-cleanup path",
                "the exact proxy remains present".to_string(),
            ));
        }
        if let Some(path) = observation.route_shadow {
            return Err(localapi_indeterminate(
                "verify post-cleanup route ownership",
                format!(
                    "Web handler {path} still overrides the journaled mount {}; the exact Ferric proxy was removed but ownership journals must remain",
                    ownership.mount_path
                ),
            ));
        }
        if observation.foreground_shadows {
            return Err(localapi_indeterminate(
                "verify post-cleanup route ownership",
                format!(
                    "foreground Serve state still shadows {}:{} after the exact Ferric proxy was removed; ownership journals must remain",
                    ownership.fqdn, ownership.https_port
                ),
            ));
        }
        if !pinned {
            return Err(localapi_indeterminate(
                "verify version-drift cleanup",
                "the exact Ferric Serve handler was removed, but unknown routing semantics prevent proving endpoint absence; ownership journals must remain"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

fn require_identity_sandwich(
    before: &[u8],
    after: &[u8],
    publication_ready: bool,
) -> Result<TailscaleIdentity, TailscaleServeError> {
    let before = parse_localapi_identity(before)?;
    let after = parse_localapi_identity(after)?;
    if before != after {
        return Err(TailscaleServeError::InvalidIdentity(
            "LocalAPI identity or HTTPS authority changed across the Serve snapshot".to_string(),
        ));
    }
    if publication_ready {
        before.require_publication_ready()?;
    } else {
        validate_stable_node_id(&before.stable_node_id)?;
        validate_fqdn(&before.fqdn)?;
    }
    Ok(before)
}

fn localapi_no_mutation(operation: &str, error: LocalApiError) -> TailscaleServeError {
    TailscaleServeError::LocalApiNoMutation(format!(
        "Tailscale LocalAPI could not {operation}; no Serve mutation was sent: {error}"
    ))
}

fn localapi_indeterminate(operation: &str, detail: String) -> TailscaleServeError {
    TailscaleServeError::LocalApiIndeterminate(format!(
        "Tailscale LocalAPI could not {operation} after a Serve mutation attempt; the journal is retained: {detail}"
    ))
}

fn map_cas_result(
    result: Result<(), ServeConfigCasError>,
    operation: &str,
) -> Result<(), TailscaleServeError> {
    match result {
        Ok(()) => Ok(()),
        Err(ServeConfigCasError::NoMutation(error)) => Err(localapi_no_mutation(operation, error)),
        Err(ServeConfigCasError::PreconditionFailed) => {
            Err(TailscaleServeError::LocalApiNoMutation(format!(
                "Tailscale LocalAPI {operation} lost its exact ETag precondition; HTTP 412 proves no mutation and no retry was attempted"
            )))
        }
        Err(ServeConfigCasError::Indeterminate(error)) => {
            Err(localapi_indeterminate(operation, error.to_string()))
        }
    }
}

pub(crate) fn parse_localapi_identity(
    raw: &[u8],
) -> Result<TailscaleIdentity, TailscaleServeError> {
    let value = parse_duplicate_safe_json(raw).map_err(|detail| {
        TailscaleServeError::InvalidIdentity(format!(
            "LocalAPI status JSON is malformed ({detail}); Tailscale 1.102.2 is required"
        ))
    })?;
    let root = value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidIdentity("LocalAPI status root must be an object".to_string())
    })?;
    let backend_running = root.get("BackendState").and_then(Value::as_str) == Some("Running");
    let self_node = root.get("Self").and_then(Value::as_object).ok_or_else(|| {
        TailscaleServeError::InvalidIdentity(
            "LocalAPI status is missing the Self object".to_string(),
        )
    })?;
    let stable_node_id = self_node.get("ID").and_then(Value::as_str).ok_or_else(|| {
        TailscaleServeError::InvalidIdentity("LocalAPI status Self.ID is missing".to_string())
    })?;
    validate_stable_node_id(stable_node_id)?;
    let raw_name = self_node
        .get("DNSName")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            TailscaleServeError::InvalidIdentity(
                "LocalAPI status Self.DNSName is missing".to_string(),
            )
        })?;
    let fqdn = raw_name.strip_suffix('.').ok_or_else(|| {
        TailscaleServeError::InvalidIdentity(
            "LocalAPI status Self.DNSName must end in exactly one trailing dot".to_string(),
        )
    })?;
    if fqdn.ends_with('.') {
        return Err(TailscaleServeError::InvalidIdentity(
            "LocalAPI status Self.DNSName has more than one trailing dot".to_string(),
        ));
    }
    validate_fqdn(fqdn)?;
    let https_capable = self_node
        .get("CapMap")
        .and_then(Value::as_object)
        .is_some_and(|capabilities| capabilities.contains_key("https"));
    let certificate_domain = root
        .get("CertDomains")
        .and_then(Value::as_array)
        .is_some_and(|domains| {
            domains
                .iter()
                .any(|candidate| candidate.as_str() == Some(fqdn))
        });
    Ok(TailscaleIdentity {
        stable_node_id: stable_node_id.to_string(),
        fqdn: fqdn.to_string(),
        backend_running,
        https_capable,
        certificate_domain,
    })
}

fn parse_localapi_serve_config(raw: &[u8]) -> Result<Value, TailscaleServeError> {
    let value = parse_localapi_serve_config_unvalidated(raw)?;
    validate_serve_config_schema(
        value
            .as_object()
            .expect("LocalAPI Serve config parser returns an object"),
        0,
    )?;
    Ok(value)
}

fn parse_localapi_serve_config_unvalidated(raw: &[u8]) -> Result<Value, TailscaleServeError> {
    let value = parse_duplicate_safe_json(raw).map_err(TailscaleServeError::InvalidStatus)?;
    match value {
        // The LocalAPI intentionally returns JSON null before the first Serve
        // config. Tailscale's own client converts that nil view to an empty
        // ServeConfig before applying a mutation.
        Value::Null => Ok(Value::Object(Map::new())),
        Value::Object(_) => Ok(value),
        _ => Err(TailscaleServeError::InvalidStatus(
            "LocalAPI Serve config must be an object or null".to_string(),
        )),
    }
}

pub(crate) fn project_localapi_status(
    raw: &[u8],
    fqdn: &str,
    mount_path: &str,
) -> Result<ServeStatusObservation, TailscaleServeError> {
    let config = parse_localapi_serve_config(raw)?;
    project_stored_coordinate(&config, fqdn, mount_path, false)
}

pub(crate) fn project_localapi_status_for_cleanup(
    raw: &[u8],
    fqdn: &str,
    mount_path: &str,
) -> Result<ServeStatusObservation, TailscaleServeError> {
    let config = parse_localapi_serve_config_unvalidated(raw)?;
    project_stored_coordinate(&config, fqdn, mount_path, true)
}

fn project_stored_coordinate(
    config: &Value,
    fqdn: &str,
    mount_path: &str,
    preserve_descendants_for_cleanup: bool,
) -> Result<ServeStatusObservation, TailscaleServeError> {
    validate_fqdn(fqdn)?;
    validate_mount_path(mount_path)?;
    let root = config.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("Serve config root must be an object".to_string())
    })?;
    let expected_host = format!("{fqdn}:{HTTPS_PORT}");
    let tcp_map_present = root.contains_key("TCP");
    let tcp_https_present = root
        .get("TCP")
        .and_then(Value::as_object)
        .is_some_and(|tcp| tcp.contains_key(&HTTPS_PORT.to_string()));
    let https_mode_compatible = compatible_https_only(root);
    let web_map_present = root.contains_key("Web");
    let foreground_shadows = foreground_shadows_coordinate(root, &expected_host)?;
    let funnel_enabled = funnel_enabled_for_host(root, &expected_host)?;
    let mut expected_web_host_present = false;
    let mut matches = Vec::new();
    let mut route_shadow = None;
    if let Some(web_value) = root.get("Web") {
        let web = web_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus("Web must be an object".to_string())
        })?;
        for (host, server_value) in web {
            if preserve_descendants_for_cleanup && host != &expected_host {
                // Cleanup authority is exactly the journaled host/path. An
                // unrelated host—even one copying the public token—cannot
                // safely block removal at the owned coordinate.
                continue;
            }
            if host == &expected_host {
                expected_web_host_present = true;
            }
            let server = server_value.as_object().ok_or_else(|| {
                TailscaleServeError::InvalidStatus(format!(
                    "Web entry {host} must be a non-null object"
                ))
            })?;
            let handlers = server
                .get("Handlers")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    TailscaleServeError::InvalidStatus(format!(
                        "Handlers for {host} must be an object"
                    ))
                })?;
            for (candidate_path, handler) in handlers {
                if candidate_path == mount_path {
                    matches.push((host.as_str(), handler));
                } else if host == &expected_host
                    && candidate_path
                        .strip_prefix(mount_path)
                        .is_some_and(|suffix| suffix.starts_with('/'))
                {
                    if preserve_descendants_for_cleanup {
                        route_shadow.get_or_insert_with(|| candidate_path.clone());
                    } else {
                        return Err(TailscaleServeError::InvalidStatus(format!(
                            "Web handler {candidate_path} is a descendant or trailing-slash alias of owned path {mount_path} and would override Ferric's advertised remote base"
                        )));
                    }
                }
            }
        }
    }
    if matches.len() > 1 {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "token path {mount_path} appears at more than one Web coordinate"
        )));
    }
    let path_state = match matches.pop() {
        None => ServePathState::Absent,
        Some((host, _)) if host != expected_host => {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "token path {mount_path} appears at unexpected Web host {host}"
            )));
        }
        Some((_, handler)) => ServePathState::Proxy {
            target: exact_proxy_target(handler, mount_path)?.to_string(),
        },
    };
    Ok(ServeStatusObservation {
        fqdn: fqdn.to_string(),
        https_port: HTTPS_PORT,
        mount_path: mount_path.to_string(),
        status_sha256: canonical_status_sha256(config),
        path_state,
        identity: None,
        scaffold: ServeScaffoldState {
            tcp_map_present,
            tcp_https_present,
            web_map_present,
            web_host_present: expected_web_host_present,
        },
        https_mode_compatible,
        funnel_enabled,
        foreground_shadows,
        route_shadow,
        cleanup_semantics_pinned: true,
    })
}

fn compatible_https_only(root: &Map<String, Value>) -> bool {
    let Some(port) = root
        .get("TCP")
        .and_then(Value::as_object)
        .and_then(|tcp| tcp.get(&HTTPS_PORT.to_string()))
        .and_then(Value::as_object)
    else {
        return false;
    };
    port.get("HTTPS").and_then(Value::as_bool) == Some(true)
        && port.iter().all(|(field, value)| match field.as_str() {
            "HTTPS" => true,
            "HTTP" | "TCPForward" | "TerminateTLS" | "ProxyProtocol" => !value_is_present(value),
            _ => false,
        })
}

fn validate_serve_config_schema(
    config: &Map<String, Value>,
    depth: usize,
) -> Result<(), TailscaleServeError> {
    if depth > 8 {
        return Err(TailscaleServeError::InvalidStatus(
            "Foreground Serve config nesting exceeds the supported bound".to_string(),
        ));
    }
    reject_unknown_fields(
        config,
        &["TCP", "Web", "Services", "AllowFunnel", "Foreground"],
        "ServeConfig",
    )?;
    if let Some(tcp) = config.get("TCP") {
        validate_tcp_map(tcp, "TCP")?;
    }
    if let Some(web) = config.get("Web") {
        validate_web_map(web, "Web")?;
    }
    if let Some(services_value) = config.get("Services") {
        let services = services_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus("Services must be an object".to_string())
        })?;
        for (name, service_value) in services {
            validate_service_name(name)?;
            let service = service_value.as_object().ok_or_else(|| {
                TailscaleServeError::InvalidStatus(format!(
                    "Services entry {name} must be a non-null object"
                ))
            })?;
            reject_unknown_fields(service, &["TCP", "Web", "Tun"], "ServiceConfig")?;
            if let Some(tcp) = service.get("TCP") {
                validate_tcp_map(tcp, &format!("Services[{name}].TCP"))?;
            }
            if let Some(web) = service.get("Web") {
                validate_web_map(web, &format!("Services[{name}].Web"))?;
            }
            let tun = match service.get("Tun") {
                Some(Value::Bool(tun)) => *tun,
                Some(_) => {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "Services entry {name} Tun must be boolean"
                    )));
                }
                None => false,
            };
            if tun
                && (service
                    .get("TCP")
                    .and_then(Value::as_object)
                    .is_some_and(|tcp| !tcp.is_empty())
                    || service
                        .get("Web")
                        .and_then(Value::as_object)
                        .is_some_and(|web| !web.is_empty()))
            {
                return Err(TailscaleServeError::InvalidStatus(format!(
                    "Services entry {name} cannot combine Tun with TCP or Web routing"
                )));
            }
        }
    }
    if let Some(funnel_value) = config.get("AllowFunnel") {
        let funnel = funnel_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus("AllowFunnel must be an object".to_string())
        })?;
        for (host, allowed) in funnel {
            validate_host_port(host)?;
            if !allowed.is_boolean() {
                return Err(TailscaleServeError::InvalidStatus(format!(
                    "AllowFunnel entry {host} must be boolean"
                )));
            }
        }
    }
    if let Some(foreground_value) = config.get("Foreground") {
        let foreground = foreground_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus("Foreground must be an object".to_string())
        })?;
        for (session, nested_value) in foreground {
            if session.is_empty() || session.chars().any(char::is_control) {
                return Err(TailscaleServeError::InvalidStatus(
                    "Foreground session keys must be nonempty printable strings".to_string(),
                ));
            }
            let nested = nested_value.as_object().ok_or_else(|| {
                TailscaleServeError::InvalidStatus(format!(
                    "Foreground session {session} must be a non-null ServeConfig object"
                ))
            })?;
            validate_serve_config_schema(nested, depth + 1)?;
        }
    }
    Ok(())
}

fn validate_tcp_map(value: &Value, label: &str) -> Result<(), TailscaleServeError> {
    let tcp = value
        .as_object()
        .ok_or_else(|| TailscaleServeError::InvalidStatus(format!("{label} must be an object")))?;
    for (port, handler_value) in tcp {
        let parsed = port.parse::<u16>().map_err(|_| {
            TailscaleServeError::InvalidStatus(format!("{label} key {port} is not a u16 port"))
        })?;
        if parsed.to_string() != *port {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "{label} key {port} is not a canonical decimal port"
            )));
        }
        let handler = handler_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus(format!(
                "{label} port {port} must be a non-null object"
            ))
        })?;
        reject_unknown_fields(
            handler,
            &[
                "HTTPS",
                "HTTP",
                "TCPForward",
                "TerminateTLS",
                "ProxyProtocol",
            ],
            "TCPPortHandler",
        )?;
        for flag in ["HTTPS", "HTTP"] {
            if handler.get(flag).is_some_and(|value| !value.is_boolean()) {
                return Err(TailscaleServeError::InvalidStatus(format!(
                    "{label} port {port} {flag} must be boolean"
                )));
            }
        }
        for text_field in ["TCPForward", "TerminateTLS"] {
            if handler
                .get(text_field)
                .is_some_and(|value| !value.is_string())
            {
                return Err(TailscaleServeError::InvalidStatus(format!(
                    "{label} port {port} {text_field} must be a string"
                )));
            }
        }
        if handler.get("ProxyProtocol").is_some_and(|value| {
            value
                .as_i64()
                .and_then(|integer| isize::try_from(integer).ok())
                .is_none()
        }) {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "{label} port {port} ProxyProtocol must be an integer"
            )));
        }
    }
    Ok(())
}

fn validate_web_map(value: &Value, label: &str) -> Result<(), TailscaleServeError> {
    let web = value
        .as_object()
        .ok_or_else(|| TailscaleServeError::InvalidStatus(format!("{label} must be an object")))?;
    for (host, server_value) in web {
        validate_host_port(host)?;
        let server = server_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus(format!(
                "{label} entry {host} must be a non-null object"
            ))
        })?;
        reject_unknown_fields(server, &["Handlers"], "WebServerConfig")?;
        let handlers = server
            .get("Handlers")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                TailscaleServeError::InvalidStatus(format!(
                    "{label} entry {host} requires a Handlers object"
                ))
            })?;
        for (mount, handler_value) in handlers {
            if !mount.starts_with('/') || mount.chars().any(char::is_control) {
                return Err(TailscaleServeError::InvalidStatus(format!(
                    "handler mount {mount:?} must be an absolute printable URL path"
                )));
            }
            let handler = handler_value.as_object().ok_or_else(|| {
                TailscaleServeError::InvalidStatus(format!(
                    "handler at {mount} must be a non-null object"
                ))
            })?;
            reject_unknown_fields(
                handler,
                &["Path", "Proxy", "Text", "AcceptAppCaps", "Redirect"],
                "HTTPHandler",
            )?;
            for field in ["Path", "Proxy", "Text", "Redirect"] {
                if handler.get(field).is_some_and(|value| !value.is_string()) {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "handler at {mount} field {field} must be a string"
                    )));
                }
            }
            if let Some(caps_value) = handler.get("AcceptAppCaps") {
                let caps = caps_value.as_array().ok_or_else(|| {
                    TailscaleServeError::InvalidStatus(format!(
                        "handler at {mount} AcceptAppCaps must be an array"
                    ))
                })?;
                if caps.iter().any(|cap| !cap.is_string()) {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "handler at {mount} AcceptAppCaps entries must be strings"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_host_port(host_port: &str) -> Result<(), TailscaleServeError> {
    let (host, port) = host_port.rsplit_once(':').ok_or_else(|| {
        TailscaleServeError::InvalidStatus(format!(
            "Web coordinate {host_port} is not a host:port value"
        ))
    })?;
    let host_syntax_valid = if let Some(literal) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        literal.parse::<std::net::Ipv6Addr>().is_ok()
    } else {
        !host.contains(':')
    };
    if host.is_empty()
        || !host_syntax_valid
        || host.chars().any(char::is_whitespace)
        || host.chars().any(char::is_control)
    {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "Web coordinate {host_port} has an invalid host"
        )));
    }
    let parsed = port.parse::<u16>().map_err(|_| {
        TailscaleServeError::InvalidStatus(format!(
            "Web coordinate {host_port} has an invalid numeric port"
        ))
    })?;
    if parsed.to_string() != port {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "Web coordinate {host_port} has a non-canonical port"
        )));
    }
    Ok(())
}

fn validate_service_name(name: &str) -> Result<(), TailscaleServeError> {
    let label = name.strip_prefix("svc:").ok_or_else(|| {
        TailscaleServeError::InvalidStatus(format!("Services key {name} must start with svc:"))
    })?;
    let valid = !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    if !valid {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "Services key {name} has an invalid DNS label"
        )));
    }
    Ok(())
}

fn reject_unknown_fields(
    object: &Map<String, Value>,
    allowed: &[&str],
    label: &str,
) -> Result<(), TailscaleServeError> {
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "{label} contains unknown field {field} for pinned Tailscale 1.102.2"
        )));
    }
    Ok(())
}

fn project_localapi_config(
    config: &Value,
    ownership: &TailscaleServeOwnership,
) -> Result<ServeStatusObservation, TailscaleServeError> {
    project_stored_coordinate(config, &ownership.fqdn, &ownership.mount_path, false)
}

fn project_localapi_config_for_cleanup(
    config: &Value,
    ownership: &TailscaleServeOwnership,
) -> Result<ServeStatusObservation, TailscaleServeError> {
    project_stored_coordinate(config, &ownership.fqdn, &ownership.mount_path, true)
}

/// Build the one whole-config CAS body that adds Ferric's exact token path.
/// Every unrelated value from the checked snapshot is retained.
pub(crate) fn prepare_localapi_apply(
    raw: &[u8],
    ownership: &TailscaleServeOwnership,
) -> Result<Vec<u8>, TailscaleServeError> {
    ownership.validate()?;
    let mut config = parse_localapi_serve_config(raw)?;
    let observation = project_localapi_config(&config, ownership)?;
    let expected_scaffold = ServeScaffoldState {
        tcp_map_present: ownership.tcp_map_preexisting,
        tcp_https_present: ownership.tcp_https_preexisting,
        web_map_present: ownership.web_map_preexisting,
        web_host_present: ownership.web_host_preexisting,
    };
    if observation.scaffold != expected_scaffold {
        return Err(TailscaleServeError::InvalidStatus(
            "Ferric's TCP/Web scaffolding changed after the authoritative journal refresh; refusing to apply with stale teardown provenance"
                .to_string(),
        ));
    }
    if let Some(hazard) = observation.preapply_hazard() {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "{hazard}; publication was refused"
        )));
    }
    match observation.owned_state(ownership)? {
        OwnedServeState::Absent => {}
        OwnedServeState::Exact => {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "owned path {} is already active before apply",
                ownership.mount_path
            )));
        }
        OwnedServeState::Replaced { observed_target } => {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "owned path {} is already claimed by {observed_target}",
                ownership.mount_path
            )));
        }
    }
    if !ownership.tcp_https_preexisting
        && any_web_uses_port(
            config
                .as_object()
                .expect("LocalAPI config parser returns an object"),
            HTTPS_PORT,
        )?
    {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "creating TCP HTTPS port {HTTPS_PORT} would activate pre-existing Web routing on that port"
        )));
    }
    if !ownership.tcp_https_preexisting
        && any_funnel_enabled_on_port(
            config
                .as_object()
                .expect("LocalAPI config parser returns an object"),
            HTTPS_PORT,
        )?
    {
        return Err(TailscaleServeError::InvalidStatus(format!(
            "creating TCP HTTPS port {HTTPS_PORT} would activate pre-existing Funnel policy on that port"
        )));
    }
    let root = config
        .as_object_mut()
        .expect("LocalAPI config parser returns an object");
    let tcp = root
        .entry("TCP".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("status validation rejects a non-object TCP map");
    tcp.entry(HTTPS_PORT.to_string())
        .or_insert_with(|| serde_json::json!({"HTTPS": true}));

    let host = format!("{}:{HTTPS_PORT}", ownership.fqdn);
    let web = root
        .entry("Web".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("status validation rejects a non-object Web map");
    let server = web
        .entry(host)
        .or_insert_with(|| serde_json::json!({"Handlers": {}}))
        .as_object_mut()
        .expect("status validation rejects a non-object owned Web server");
    let handlers = server
        .entry("Handlers".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .expect("status validation rejects a non-object Handlers map");
    handlers.insert(
        ownership.mount_path.clone(),
        serde_json::json!({"Proxy": ownership.proxy_target}),
    );
    serde_json::to_vec(&config).map_err(|error| {
        TailscaleServeError::InvalidStatus(format!(
            "could not serialize the exact LocalAPI apply body: {error}"
        ))
    })
}

/// Build the one whole-config CAS body that removes only Ferric's exact path.
/// `None` means the checked snapshot is already absent and needs no POST.
pub(crate) fn prepare_localapi_off(
    raw: &[u8],
    ownership: &TailscaleServeOwnership,
) -> Result<Option<Vec<u8>>, TailscaleServeError> {
    prepare_localapi_off_inner(raw, ownership, true)
}

pub(crate) fn prepare_localapi_off_preserving_scaffold(
    raw: &[u8],
    ownership: &TailscaleServeOwnership,
) -> Result<Option<Vec<u8>>, TailscaleServeError> {
    prepare_localapi_off_inner(raw, ownership, false)
}

fn prepare_localapi_off_inner(
    raw: &[u8],
    ownership: &TailscaleServeOwnership,
    prune_pinned_scaffolding: bool,
) -> Result<Option<Vec<u8>>, TailscaleServeError> {
    ownership.validate()?;
    let mut config = parse_localapi_serve_config_unvalidated(raw)?;
    match project_localapi_config_for_cleanup(&config, ownership)?.owned_state(ownership)? {
        OwnedServeState::Absent => return Ok(None),
        OwnedServeState::Exact => {}
        OwnedServeState::Replaced { observed_target } => {
            return Err(TailscaleServeError::InvalidStatus(format!(
                "owned path {} now targets {observed_target}; LocalAPI removal was refused",
                ownership.mount_path
            )));
        }
    }
    let root = config
        .as_object_mut()
        .expect("LocalAPI config parser returns an object");
    let host = format!("{}:{HTTPS_PORT}", ownership.fqdn);
    let host_is_exact_empty_scaffold = {
        let server = root
            .get_mut("Web")
            .and_then(Value::as_object_mut)
            .and_then(|web| web.get_mut(&host))
            .and_then(Value::as_object_mut)
            .expect("exact projection proves the owned Web server");
        let handlers = server
            .get_mut("Handlers")
            .and_then(Value::as_object_mut)
            .expect("exact projection proves the owned Handlers map");
        handlers.remove(&ownership.mount_path);
        handlers.is_empty() && server.len() == 1
    };
    if prune_pinned_scaffolding && host_is_exact_empty_scaffold && !ownership.web_host_preexisting {
        let web_became_empty = {
            let web = root
                .get_mut("Web")
                .and_then(Value::as_object_mut)
                .expect("exact projection proves the Web map");
            web.remove(&host);
            web.is_empty()
        };
        if web_became_empty && !ownership.web_map_preexisting {
            root.remove("Web");
        }
    }
    if prune_pinned_scaffolding
        && !ownership.tcp_https_preexisting
        && tcp_port_is_exact_created_https_scaffold(root)
        && !any_web_uses_port(root, HTTPS_PORT)?
        && !any_funnel_enabled_on_port(root, HTTPS_PORT)?
    {
        let tcp_became_empty = root
            .get_mut("TCP")
            .and_then(Value::as_object_mut)
            .is_some_and(|tcp| {
                tcp.remove(&HTTPS_PORT.to_string());
                tcp.is_empty()
            });
        if tcp_became_empty && !ownership.tcp_map_preexisting {
            root.remove("TCP");
        }
    }
    let body = serde_json::to_vec(&config).map_err(|error| {
        TailscaleServeError::InvalidStatus(format!(
            "could not serialize the exact LocalAPI removal body: {error}"
        ))
    })?;
    if !prune_pinned_scaffolding && json_number_lexemes(raw) != json_number_lexemes(&body) {
        return Err(TailscaleServeError::InvalidStatus(
            "version-drift cleanup refused because reserialization would alter an unknown JSON number"
                .to_string(),
        ));
    }
    Ok(Some(body))
}

/// Return exact JSON number tokens in encounter order. The input has already
/// passed the duplicate-safe JSON parser, so a small lexical pass is enough to
/// distinguish numbers from identical bytes inside strings. Forward-version
/// cleanup compares these before and after mutation to prevent f64 rounding of
/// unknown fields when serde_json lacks `arbitrary_precision`.
fn json_number_lexemes(raw: &[u8]) -> Vec<&[u8]> {
    let mut numbers = Vec::new();
    let mut index = 0;
    while index < raw.len() {
        match raw[index] {
            b'"' => {
                index += 1;
                while index < raw.len() {
                    match raw[index] {
                        b'\\' => index = (index + 2).min(raw.len()),
                        b'"' => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = index;
                index += 1;
                while index < raw.len()
                    && matches!(raw[index], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    index += 1;
                }
                numbers.push(&raw[start..index]);
            }
            _ => index += 1,
        }
    }
    numbers
}

fn tcp_port_is_exact_created_https_scaffold(root: &Map<String, Value>) -> bool {
    root.get("TCP")
        .and_then(Value::as_object)
        .and_then(|tcp| tcp.get(&HTTPS_PORT.to_string()))
        .and_then(Value::as_object)
        .is_some_and(|handler| {
            handler.len() == 1 && handler.get("HTTPS").and_then(Value::as_bool) == Some(true)
        })
}

fn top_level_web_uses_port(
    root: &Map<String, Value>,
    expected_port: u16,
) -> Result<bool, TailscaleServeError> {
    let Some(web_value) = root.get("Web") else {
        return Ok(false);
    };
    let web = web_value
        .as_object()
        .ok_or_else(|| TailscaleServeError::InvalidStatus("Web must be an object".to_string()))?;
    for host in web.keys() {
        let (_, raw_port) = host.rsplit_once(':').ok_or_else(|| {
            TailscaleServeError::InvalidStatus(format!(
                "Web coordinate {host} is not a host:port value"
            ))
        })?;
        let port = raw_port.parse::<u16>().map_err(|_| {
            TailscaleServeError::InvalidStatus(format!(
                "Web coordinate {host} has an invalid numeric port"
            ))
        })?;
        if port == expected_port {
            return Ok(true);
        }
    }
    Ok(false)
}

fn any_web_uses_port(
    root: &Map<String, Value>,
    expected_port: u16,
) -> Result<bool, TailscaleServeError> {
    if top_level_web_uses_port(root, expected_port)? {
        return Ok(true);
    }
    let Some(foreground_value) = root.get("Foreground") else {
        return Ok(false);
    };
    let foreground = foreground_value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("Foreground must be an object".to_string())
    })?;
    for (session, nested_value) in foreground {
        let nested = nested_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus(format!(
                "Foreground session {session} must be a Serve config object"
            ))
        })?;
        if any_web_uses_port(nested, expected_port)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn funnel_enabled_for_host(
    root: &Map<String, Value>,
    expected_host: &str,
) -> Result<bool, TailscaleServeError> {
    if root
        .get("AllowFunnel")
        .and_then(Value::as_object)
        .and_then(|funnel| funnel.get(expected_host))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return Ok(true);
    }
    let Some(foreground_value) = root.get("Foreground") else {
        return Ok(false);
    };
    let foreground = foreground_value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("Foreground must be an object".to_string())
    })?;
    for (session, nested_value) in foreground {
        let nested = nested_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus(format!(
                "Foreground session {session} must be a Serve config object"
            ))
        })?;
        if funnel_enabled_for_host(nested, expected_host)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn any_funnel_enabled_on_port(
    root: &Map<String, Value>,
    expected_port: u16,
) -> Result<bool, TailscaleServeError> {
    if let Some(funnel_value) = root.get("AllowFunnel") {
        let funnel = funnel_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus("AllowFunnel must be an object".to_string())
        })?;
        for (host, enabled) in funnel {
            if enabled.as_bool() == Some(true) {
                let (_, raw_port) = host.rsplit_once(':').ok_or_else(|| {
                    TailscaleServeError::InvalidStatus(format!(
                        "AllowFunnel coordinate {host} is not a host:port value"
                    ))
                })?;
                if raw_port.parse::<u16>() == Ok(expected_port) {
                    return Ok(true);
                }
            }
        }
    }
    let Some(foreground_value) = root.get("Foreground") else {
        return Ok(false);
    };
    let foreground = foreground_value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("Foreground must be an object".to_string())
    })?;
    for (session, nested_value) in foreground {
        let nested = nested_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus(format!(
                "Foreground session {session} must be a Serve config object"
            ))
        })?;
        if any_funnel_enabled_on_port(nested, expected_port)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn foreground_shadows_coordinate(
    root: &Map<String, Value>,
    expected_host: &str,
) -> Result<bool, TailscaleServeError> {
    let Some(foreground_value) = root.get("Foreground") else {
        return Ok(false);
    };
    let foreground = foreground_value.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus("Foreground must be an object when present".to_string())
    })?;
    for (session, config_value) in foreground {
        let config = config_value.as_object().ok_or_else(|| {
            TailscaleServeError::InvalidStatus(format!(
                "Foreground session {session} must contain a Serve config object"
            ))
        })?;
        let shadows_tcp = config
            .get("TCP")
            .and_then(Value::as_object)
            .is_some_and(|tcp| tcp.contains_key(&HTTPS_PORT.to_string()));
        let shadows_web = config
            .get("Web")
            .and_then(Value::as_object)
            .is_some_and(|web| web.contains_key(expected_host));
        if shadows_tcp || shadows_web {
            return Ok(true);
        }
    }
    Ok(false)
}

fn exact_proxy_target<'a>(
    handler: &'a Value,
    mount_path: &str,
) -> Result<&'a str, TailscaleServeError> {
    let object = handler.as_object().ok_or_else(|| {
        TailscaleServeError::InvalidStatus(format!("handler at {mount_path} must be an object"))
    })?;
    let proxy = object.get("Proxy").and_then(Value::as_str).ok_or_else(|| {
        TailscaleServeError::InvalidStatus(format!(
            "handler at {mount_path} is not an exact proxy handler"
        ))
    })?;
    for (field, value) in object {
        match field.as_str() {
            "Proxy" => {}
            "Text" | "Path" | "AcceptAppCaps" | "Redirect" => {
                if value_is_present(value) {
                    return Err(TailscaleServeError::InvalidStatus(format!(
                        "handler at {mount_path} combines proxy with non-proxy {field} behavior"
                    )));
                }
            }
            _ => {
                return Err(TailscaleServeError::InvalidStatus(format!(
                    "handler at {mount_path} contains unknown field {field}"
                )));
            }
        }
    }
    Ok(proxy)
}

fn value_is_present(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        Value::Bool(true) | Value::Number(_) => true,
    }
}

fn proxy_target_for_port(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn proxy_target_port(target: &str) -> Result<u16, TailscaleServeError> {
    let raw_port = target.strip_prefix("http://127.0.0.1:").ok_or_else(|| {
        TailscaleServeError::InvalidOwnership(
            "proxy_target must use exact loopback origin http://127.0.0.1:<port>".to_string(),
        )
    })?;
    let port = raw_port.parse::<u16>().map_err(|_| {
        TailscaleServeError::InvalidOwnership(
            "proxy_target must end in a canonical nonzero u16 port".to_string(),
        )
    })?;
    if port == 0 || raw_port != port.to_string() {
        return Err(TailscaleServeError::InvalidOwnership(
            "proxy_target must end in a canonical nonzero u16 port".to_string(),
        ));
    }
    Ok(port)
}

fn remote_base_for(fqdn: &str, mount_path: &str) -> String {
    format!("https://{fqdn}{mount_path}/v1")
}

fn mount_path_for_token(token: &str) -> Result<String, TailscaleServeError> {
    validate_token(token)?;
    Ok(format!("/_ferric/{token}"))
}

fn validate_mount_path(path: &str) -> Result<(), TailscaleServeError> {
    let token = path.strip_prefix("/_ferric/").ok_or_else(|| {
        TailscaleServeError::InvalidOwnership(
            "mount path must use /_ferric/<32-lowercase-hex>".to_string(),
        )
    })?;
    if token.contains('/') {
        return Err(TailscaleServeError::InvalidOwnership(
            "mount path must name exactly one token segment".to_string(),
        ));
    }
    validate_token(token)
}

fn validate_token(token: &str) -> Result<(), TailscaleServeError> {
    validate_lower_hex("token", token, TOKEN_HEX_LEN)
}

fn validate_lower_hex(
    label: &str,
    value: &str,
    expected_len: usize,
) -> Result<(), TailscaleServeError> {
    if value.len() != expected_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(TailscaleServeError::InvalidOwnership(format!(
            "{label} must be exactly {expected_len} lowercase hexadecimal characters"
        )));
    }
    Ok(())
}

fn validate_stable_node_id(stable_node_id: &str) -> Result<(), TailscaleServeError> {
    // tailcfg.StableNodeID is intentionally opaque. Bound it and reject
    // whitespace/control bytes without inventing a prefix grammar that the
    // upstream type does not promise.
    if stable_node_id.is_empty()
        || stable_node_id.len() > 256
        || !stable_node_id.is_ascii()
        || stable_node_id
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
    {
        return Err(TailscaleServeError::InvalidIdentity(
            "Self.ID must be a nonempty bounded opaque ASCII stable node ID".to_string(),
        ));
    }
    Ok(())
}

fn validate_fqdn(fqdn: &str) -> Result<(), TailscaleServeError> {
    if fqdn.is_empty()
        || fqdn.len() > 253
        || fqdn.ends_with('.')
        || fqdn.bytes().any(|byte| !byte.is_ascii())
        || fqdn.bytes().any(|byte| byte.is_ascii_uppercase())
    {
        return Err(TailscaleServeError::InvalidIdentity(
            "Node.Name must be a canonical lowercase ASCII FQDN without a trailing dot".to_string(),
        ));
    }
    let labels = fqdn.split('.').collect::<Vec<_>>();
    if labels.len() < 2
        || labels.iter().any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(TailscaleServeError::InvalidIdentity(
            "Node.Name must be a canonical lowercase DNS name".to_string(),
        ));
    }
    Ok(())
}

fn canonical_status_sha256(value: &Value) -> String {
    let canonical = canonicalize_value(value);
    let bytes = serde_json::to_vec(&canonical).expect("serde_json::Value always serializes");
    hex::encode(Sha256::digest(bytes))
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_value).collect()),
        Value::Object(object) => {
            let sorted = object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize_value(value)))
                .collect::<BTreeMap<_, _>>();
            let mut canonical = Map::new();
            for (key, value) in sorted {
                canonical.insert(key, value);
            }
            Value::Object(canonical)
        }
        scalar => scalar.clone(),
    }
}

/// Deserialize JSON while rejecting duplicate keys at every depth. Ordinary
/// `serde_json::Value` parsing retains only the last duplicate, which would
/// erase precisely the ambiguity this adapter must fail closed on.
fn parse_duplicate_safe_json(raw: &[u8]) -> Result<Value, String> {
    serde_json::from_slice::<DuplicateSafeValue>(raw)
        .map(|value| value.0)
        .map_err(|error| error.to_string())
}

struct DuplicateSafeValue(Value);

impl<'de> Deserialize<'de> for DuplicateSafeValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(DuplicateSafeVisitor)
    }
}

struct DuplicateSafeVisitor;

impl<'de> Visitor<'de> for DuplicateSafeVisitor {
    type Value = DuplicateSafeValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .map(DuplicateSafeValue)
            .ok_or_else(|| E::custom("non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.visit_string(value.to_string())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(DuplicateSafeValue(Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        DuplicateSafeValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<DuplicateSafeValue>()? {
            values.push(value.0);
        }
        Ok(DuplicateSafeValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut object = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate object key {key:?}")));
            }
            let value = map.next_value::<DuplicateSafeValue>()?;
            object.insert(key, value.0);
        }
        Ok(DuplicateSafeValue(Value::Object(object)))
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::thread;
    use std::time::Duration;

    use super::*;

    const FQDN: &str = "example-host.tailnet-example.ts.net";
    const TOKEN: &str = "00112233445566778899aabbccddeeff";
    const MOUNT: &str = "/_ferric/00112233445566778899aabbccddeeff";

    #[test]
    fn cleanup_reopens_with_bounded_version_drift_after_first_response() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let address = listener.local_addr().expect("listener address");
        let status = format!(
            r#"{{"BackendState":"Running","CertDomains":["{FQDN}"],"Self":{{"DNSName":"{FQDN}.","ID":"n-stable","NodeID":123,"CapMap":{{"https":null}}}}}}"#
        );
        let server = thread::spawn(move || {
            for connection_index in 0..2 {
                let (stream, _) = listener.accept().expect("LocalAPI connection");
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .expect("read timeout");
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("request header");
                    assert!(!line.is_empty(), "unexpected request EOF");
                    request.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                assert!(request.starts_with("GET /localapi/v0/status?peers=false HTTP/1.1\r\n"));
                write!(
                    reader.get_mut(),
                    "HTTP/1.1 200 OK\r\nTailscale-Cap: 143\r\nTailscale-Version: 1.103.0\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    status.len(),
                    status
                )
                .expect("status response");
                if connection_index == 0 {
                    // Exact mode rejects this response. The adapter must close
                    // it and establish a new cleanup-mode connection.
                    continue;
                }
            }
        });

        let client = TailscaleLocalApiClient::with_test_tcp(address).expect("test LocalAPI");
        let adapter = TailscaleServeAdapter::with_client(client);
        let (session, observed, pinned) = adapter.cleanup_session().expect("cleanup fallback");
        assert!(!pinned, "future daemon must use conservative cleanup mode");
        assert_eq!(observed.self_stable_id(), "n-stable");
        drop(session);
        server.join().expect("server joined");
    }

    struct FixedEntropy(Result<[u8; TOKEN_BYTES], &'static str>);

    impl EntropySource for FixedEntropy {
        fn fill_128(&self, destination: &mut [u8; TOKEN_BYTES]) -> Result<(), TailscaleServeError> {
            match self.0 {
                Ok(bytes) => {
                    *destination = bytes;
                    Ok(())
                }
                Err(detail) => Err(TailscaleServeError::Entropy(detail.to_string())),
            }
        }
    }

    fn ownership() -> TailscaleServeOwnership {
        TailscaleServeOwnership {
            version: OWNERSHIP_VERSION,
            token: TOKEN.to_string(),
            stable_node_id: "node-fixture".to_string(),
            fqdn: FQDN.to_string(),
            https_port: HTTPS_PORT,
            mount_path: MOUNT.to_string(),
            proxy_target: "http://127.0.0.1:8080".to_string(),
            remote_base_url: format!("https://{FQDN}{MOUNT}/v1"),
            before_status_sha256: "a".repeat(64),
            tcp_map_preexisting: false,
            tcp_https_preexisting: false,
            web_map_preexisting: false,
            web_host_preexisting: false,
            apply_confirmed: false,
        }
    }

    fn identity() -> TailscaleIdentity {
        TailscaleIdentity {
            stable_node_id: "node-fixture".to_string(),
            fqdn: FQDN.to_string(),
            backend_running: true,
            https_capable: true,
            certificate_domain: true,
        }
    }

    fn ownership_for_config(raw: &[u8]) -> TailscaleServeOwnership {
        let coordinate = coordinate_from_token(8080, &identity(), TOKEN.to_string()).unwrap();
        let mut observation = project_localapi_status(raw, FQDN, MOUNT).unwrap();
        observation.identity = Some(identity());
        coordinate.into_ownership(&observation).unwrap()
    }

    fn json(raw: &[u8]) -> Value {
        serde_json::from_slice(raw).unwrap()
    }

    #[test]
    fn localapi_identity_binds_publication_and_allows_same_node_cleanup() {
        let raw = format!(
            r#"{{"BackendState":"Running","Self":{{"ID":"node-fixture","DNSName":"{FQDN}.","CapMap":{{"https":[]}}}},"CertDomains":["{FQDN}"]}}"#
        );
        let current = parse_localapi_identity(raw.as_bytes()).unwrap();
        let ownership = ownership_for_config(b"null");
        current
            .require_same_publication_identity(&ownership)
            .unwrap();

        let renamed = TailscaleIdentity {
            fqdn: "renamed.tailnet-example.ts.net".to_string(),
            backend_running: false,
            https_capable: false,
            certificate_domain: false,
            ..current.clone()
        };
        assert!(
            renamed
                .require_same_publication_identity(&ownership)
                .is_err()
        );
        renamed.require_cleanup_identity(&ownership).unwrap();

        let switched = TailscaleIdentity {
            stable_node_id: "other-node".to_string(),
            ..renamed
        };
        assert!(switched.require_cleanup_identity(&ownership).is_err());
    }

    #[test]
    fn localapi_apply_and_off_round_trip_pristine_config() {
        let ownership = ownership_for_config(b"null");
        let applied = prepare_localapi_apply(b"null", &ownership).unwrap();
        let applied_value = json(&applied);
        assert_eq!(
            applied_value["TCP"]["443"],
            serde_json::json!({"HTTPS": true})
        );
        assert_eq!(
            applied_value["Web"][format!("{FQDN}:443")]["Handlers"][MOUNT],
            serde_json::json!({"Proxy": "http://127.0.0.1:8080"})
        );

        let removed = prepare_localapi_off(&applied, &ownership)
            .unwrap()
            .expect("exact path requires removal");
        assert_eq!(json(&removed), serde_json::json!({}));
    }

    #[test]
    fn localapi_apply_preserves_supported_unrelated_state() {
        let initial = format!(
            r#"{{"TCP":{{"443":{{"HTTPS":true}},"8443":{{"HTTP":true}}}},"Web":{{"{FQDN}:443":{{"Handlers":{{"/operator":{{"Text":"kept"}}}}}},"other.tailnet-example.ts.net:443":{{"Handlers":{{"/other":{{"Proxy":"http://127.0.0.1:9"}}}}}}}},"Services":{{"svc:demo":{{"TCP":{{"9000":{{"TCPForward":"127.0.0.1:9"}}}},"Tun":false}}}},"AllowFunnel":{{"{FQDN}:443":false,"other.tailnet-example.ts.net:443":true}},"Foreground":{{"session":{{"TCP":{{"8443":{{"HTTPS":true}}}},"Web":{{"foreground.tailnet-example.ts.net:8443":{{"Handlers":{{"/fg":{{"Text":"kept"}}}}}}}}}}}}}}"#
        );
        let ownership = ownership_for_config(initial.as_bytes());
        let applied = json(&prepare_localapi_apply(initial.as_bytes(), &ownership).unwrap());
        let before = json(initial.as_bytes());
        for field in ["Services", "AllowFunnel", "Foreground"] {
            assert_eq!(applied[field], before[field], "field={field}");
        }
        assert_eq!(applied["TCP"]["8443"], before["TCP"]["8443"]);
        assert_eq!(
            applied["Web"]["other.tailnet-example.ts.net:443"],
            before["Web"]["other.tailnet-example.ts.net:443"]
        );
        assert_eq!(
            applied["Web"][format!("{FQDN}:443")]["Handlers"]["/operator"],
            before["Web"][format!("{FQDN}:443")]["Handlers"]["/operator"]
        );
    }

    #[test]
    fn localapi_apply_rejects_activation_and_schema_hazards() {
        let cases = [
            r#"{"TCP":{"443":{"HTTP":true}}}"#.to_string(),
            format!(r#"{{"TCP":{{"443":{{"HTTPS":true}}}},"AllowFunnel":{{"{FQDN}:443":true}}}}"#),
            format!(
                r#"{{"TCP":{{"443":{{"HTTPS":true}}}},"Foreground":{{"session":{{"Web":{{"{FQDN}:443":{{"Handlers":{{"/fg":{{"Text":"shadow"}}}}}}}}}}}}}}"#
            ),
            r#"{"Future":{"unsafe":true}}"#.to_string(),
            r#"{"Web":{"other.tailnet-example.ts.net:443":{"Handlers":{"/staged":{"Text":"would activate"}}}}}"#.to_string(),
            format!(
                r#"{{"Web":{{"{FQDN}:443":{{"Handlers":{{"{MOUNT}/":{{"Text":"alias"}}}}}}}},"TCP":{{"443":{{"HTTPS":true}}}}}}"#
            ),
        ];
        for (index, config) in cases.into_iter().enumerate() {
            let coordinate = coordinate_from_token(8080, &identity(), TOKEN.to_string()).unwrap();
            let result = project_localapi_status(config.as_bytes(), FQDN, MOUNT)
                .and_then(|mut observation| {
                    observation.identity = Some(identity());
                    coordinate.into_ownership(&observation)
                })
                .and_then(|ownership| {
                    prepare_localapi_apply(config.as_bytes(), &ownership).map(drop)
                });
            assert!(result.is_err(), "case {index} unexpectedly succeeded");
        }
    }

    #[test]
    fn localapi_descendant_blocks_publication_but_cleanup_preserves_it() {
        let descendant = format!("{MOUNT}/v1");
        let descendant_only = serde_json::json!({
            "TCP": {"443": {"HTTPS": true}},
            "Web": {
                format!("{FQDN}:443"): {
                    "Handlers": {
                        descendant.clone(): {"Text": "route takeover"}
                    }
                }
            }
        });
        let descendant_only = serde_json::to_vec(&descendant_only).unwrap();
        let error = prepare_localapi_apply(&descendant_only, &ownership())
            .expect_err("a descendant route must block pre-apply publication");
        assert!(error.to_string().contains("descendant"));

        let ownership = ownership_for_config(b"{}");
        let mut active = json(&prepare_localapi_apply(b"{}", &ownership).unwrap());
        active["Web"][format!("{FQDN}:443")]["Handlers"][&descendant] =
            serde_json::json!({"Text": "must survive scoped cleanup"});
        let active = serde_json::to_vec(&active).unwrap();

        let error = project_localapi_status(&active, FQDN, MOUNT)
            .expect_err("a descendant route must block active status");
        assert!(error.to_string().contains("descendant"));

        let removed = prepare_localapi_off(&active, &ownership)
            .unwrap()
            .expect("the exact Ferric parent still requires removal");
        let removed = json(&removed);
        assert!(
            removed["Web"][format!("{FQDN}:443")]["Handlers"]
                .get(MOUNT)
                .is_none()
        );
        assert_eq!(
            removed["Web"][format!("{FQDN}:443")]["Handlers"][&descendant],
            serde_json::json!({"Text": "must survive scoped cleanup"})
        );
        assert_eq!(removed["TCP"]["443"], serde_json::json!({"HTTPS": true}));

        let removed = serde_json::to_vec(&removed).unwrap();
        let projected = project_localapi_status_for_cleanup(&removed, FQDN, MOUNT).unwrap();
        assert_eq!(projected.path_state, ServePathState::Absent);
        assert_eq!(projected.route_shadow.as_deref(), Some(descendant.as_str()));
    }

    #[test]
    fn localapi_cleanup_preserves_alias_and_unrelated_host_token() {
        let ownership = ownership_for_config(b"{}");
        let mut active = json(&prepare_localapi_apply(b"{}", &ownership).unwrap());
        let alias = format!("{MOUNT}/");
        active["Web"][format!("{FQDN}:443")]["Handlers"][&alias] =
            serde_json::json!({"Text": "expected-host alias"});
        active["Web"]["other.tailnet-example.ts.net:443"] = serde_json::json!({
            "Handlers": {MOUNT: {"Text": "unrelated host mirror"}}
        });
        let active = serde_json::to_vec(&active).unwrap();

        let removed = prepare_localapi_off(&active, &ownership)
            .unwrap()
            .expect("the exact expected-host parent requires removal");
        let removed = json(&removed);
        assert!(
            removed["Web"][format!("{FQDN}:443")]["Handlers"]
                .get(MOUNT)
                .is_none()
        );
        assert_eq!(
            removed["Web"][format!("{FQDN}:443")]["Handlers"][&alias],
            serde_json::json!({"Text": "expected-host alias"})
        );
        assert_eq!(
            removed["Web"]["other.tailnet-example.ts.net:443"]["Handlers"][MOUNT],
            serde_json::json!({"Text": "unrelated host mirror"})
        );

        let projected = project_localapi_status_for_cleanup(
            &serde_json::to_vec(&removed).unwrap(),
            FQDN,
            MOUNT,
        )
        .unwrap();
        assert_eq!(projected.path_state, ServePathState::Absent);
        assert_eq!(projected.route_shadow.as_deref(), Some(alias.as_str()));
    }

    #[test]
    fn localapi_off_preserves_preexisting_and_concurrent_scaffolding() {
        let initial = format!(
            r#"{{"TCP":{{"443":{{"HTTPS":true}}}},"Web":{{"{FQDN}:443":{{"Handlers":{{}}}}}}}}"#
        );
        let ownership = ownership_for_config(initial.as_bytes());
        assert!(ownership.tcp_https_preexisting);
        assert!(ownership.web_host_preexisting);
        let applied = prepare_localapi_apply(initial.as_bytes(), &ownership).unwrap();
        let removed = prepare_localapi_off(&applied, &ownership)
            .unwrap()
            .expect("exact path is present");
        assert_eq!(json(&removed), json(initial.as_bytes()));

        let fresh = ownership_for_config(b"{}");
        let mut concurrent = json(&prepare_localapi_apply(b"{}", &fresh).unwrap());
        concurrent["TCP"]["443"] = serde_json::json!({"HTTPS": true, "HTTP": true});
        concurrent["Web"][format!("{FQDN}:443")]["FutureHostField"] =
            serde_json::json!({"kept": true});
        concurrent["Foreground"] = serde_json::json!({
            "session": {
                "Web": {
                    "foreground.tailnet-example.ts.net:443": {
                        "Handlers": {"/fg": {"Text": "kept"}}
                    }
                }
            }
        });
        concurrent["AllowFunnel"] = serde_json::json!({"other.tailnet-example.ts.net:443": true});
        concurrent["FutureRoot"] = serde_json::json!({"kept": true});
        let concurrent_raw = serde_json::to_vec(&concurrent).unwrap();
        let removed = json(
            &prepare_localapi_off(&concurrent_raw, &fresh)
                .unwrap()
                .expect("exact path is present"),
        );
        assert!(
            removed["Web"][format!("{FQDN}:443")]["Handlers"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert_eq!(removed["TCP"]["443"], concurrent["TCP"]["443"]);
        for field in ["Foreground", "AllowFunnel", "FutureRoot"] {
            assert_eq!(removed[field], concurrent[field], "field={field}");
        }
    }

    #[test]
    fn localapi_off_keeps_created_https_listener_for_each_live_dependency() {
        let ownership = ownership_for_config(b"{}");

        let mut foreground = json(&prepare_localapi_apply(b"{}", &ownership).unwrap());
        foreground["Foreground"] = serde_json::json!({
            "session": {
                "Web": {
                    "foreground.tailnet-example.ts.net:443": {
                        "Handlers": {"/operator": {"Text": "kept"}}
                    }
                }
            }
        });
        let removed = json(
            &prepare_localapi_off(&serde_json::to_vec(&foreground).unwrap(), &ownership)
                .unwrap()
                .expect("exact parent requires removal"),
        );
        assert_eq!(removed["TCP"]["443"], serde_json::json!({"HTTPS": true}));
        assert_eq!(removed["Foreground"], foreground["Foreground"]);

        let mut funnel = json(&prepare_localapi_apply(b"{}", &ownership).unwrap());
        funnel["AllowFunnel"] = serde_json::json!({
            "other.tailnet-example.ts.net:443": true
        });
        let removed = json(
            &prepare_localapi_off(&serde_json::to_vec(&funnel).unwrap(), &ownership)
                .unwrap()
                .expect("exact parent requires removal"),
        );
        assert_eq!(removed["TCP"]["443"], serde_json::json!({"HTTPS": true}));
        assert_eq!(removed["AllowFunnel"], funnel["AllowFunnel"]);
    }

    #[test]
    fn version_drift_cleanup_removes_only_handler_and_never_scaffolding() {
        let ownership = ownership_for_config(b"{}");
        let mut active = json(&prepare_localapi_apply(b"{}", &ownership).unwrap());
        active["FutureRoot"] = serde_json::json!({"opaque": "kept"});
        let body = prepare_localapi_off_preserving_scaffold(
            &serde_json::to_vec(&active).unwrap(),
            &ownership,
        )
        .unwrap()
        .expect("exact parent requires removal");
        let removed = json(&body);
        assert_eq!(removed["TCP"]["443"], serde_json::json!({"HTTPS": true}));
        assert!(
            removed["Web"][format!("{FQDN}:443")]["Handlers"]
                .as_object()
                .unwrap()
                .is_empty()
        );
        assert_eq!(removed["FutureRoot"], active["FutureRoot"]);
    }

    #[test]
    fn forward_cleanup_never_rewrites_unknown_number_lexemes() {
        let ownership = ownership_for_config(b"{}");
        let config_with = |number: &str| {
            format!(
                r#"{{"TCP":{{"443":{{"HTTPS":true}}}},"Web":{{"{FQDN}:443":{{"Handlers":{{"{MOUNT}":{{"Proxy":"http://127.0.0.1:8080"}}}}}}}},"FutureNumeric":{number}}}"#
            )
        };

        let exact_integer = config_with("123");
        let removed =
            prepare_localapi_off_preserving_scaffold(exact_integer.as_bytes(), &ownership)
                .expect("exact integer lexeme is preserved")
                .expect("owned handler is present");
        assert_eq!(
            json_number_lexemes(&removed),
            vec![&b"123"[..]],
            "the future number must be byte-identical"
        );

        let high_precision =
            config_with("123456789012345678901234567890.123456789012345678901234567890");
        let error = prepare_localapi_off_preserving_scaffold(high_precision.as_bytes(), &ownership)
            .expect_err("lossy future number must refuse cleanup");
        assert!(error.to_string().contains("unknown JSON number"));
    }

    #[test]
    fn localapi_off_refuses_replaced_target() {
        let ownership = ownership_for_config(b"{}");
        let mut applied = json(&prepare_localapi_apply(b"{}", &ownership).unwrap());
        applied["Web"][format!("{FQDN}:443")]["Handlers"][MOUNT]["Proxy"] =
            Value::String("http://127.0.0.1:9999".to_string());
        assert!(prepare_localapi_off(&serde_json::to_vec(&applied).unwrap(), &ownership).is_err());
    }

    #[test]
    fn ownership_token_and_remote_base_are_valid() {
        let bytes = [
            0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ];
        let coordinate =
            prepare_coordinate_with_entropy(8080, &identity(), &FixedEntropy(Ok(bytes))).unwrap();
        assert_eq!(coordinate.token, TOKEN);
        assert_eq!(coordinate.mount_path, MOUNT);
        assert_eq!(coordinate.proxy_target, "http://127.0.0.1:8080");
        assert_eq!(
            coordinate.remote_base_url,
            format!("https://{FQDN}{MOUNT}/v1")
        );
        let mut observation = project_localapi_status(b"{}", FQDN, MOUNT).unwrap();
        observation.identity = Some(identity());
        let ownership = coordinate.into_ownership(&observation).unwrap();
        ownership.validate_for_port(8080).unwrap();
        assert_eq!(ownership.proxy_port().unwrap(), 8080);
    }

    #[test]
    fn ownership_entropy_failure_precedes_side_effects() {
        let error = generate_token_with_entropy(&FixedEntropy(Err("injected entropy failure")))
            .unwrap_err();
        assert!(error.to_string().contains("injected entropy failure"));
    }
}
