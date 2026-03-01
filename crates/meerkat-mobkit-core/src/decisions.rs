use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::types::{ModuleConfig, RestartPolicy};

pub const REQUIRED_RELEASE_TARGETS: &[&str] = &["crates.io", "npm", "pypi", "github-releases"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecisionPolicyError {
    EmptyBigQueryDataset,
    EmptyBigQueryTable,
    InvalidBigQueryName(String),
    TomlParse(String),
    MissingModuleId,
    MissingModuleCommand,
    AuthProviderMismatch,
    AuthProviderNotSupported,
    EmailNotAllowlisted,
    InvalidServiceIdentity,
    ServiceIdentityNotAllowlisted,
    ReplicaCountMustBeOne(u16),
    SloTargetsNotSupportedV01,
    MissingReleaseTarget(String),
    DuplicateReleaseTarget(String),
    InvalidSupportMatrix(String),
    InvalidTrustedAuthConfig(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BigQueryNaming {
    pub dataset: String,
    pub table: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TrustedMobkitToml {
    pub modules: Vec<TrustedModuleDecl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TrustedModuleDecl {
    pub id: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub restart_policy: Option<RestartPolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthProvider {
    GoogleOAuth,
    GitHubOAuth,
    GenericOidc,
    ServiceIdentity,
    TestProvider,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthPolicy {
    pub default_provider: AuthProvider,
    pub email_allowlist: Vec<String>,
}

impl Default for AuthPolicy {
    fn default() -> Self {
        Self {
            default_provider: AuthProvider::GoogleOAuth,
            email_allowlist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsolePolicy {
    pub require_app_auth: bool,
}

impl Default for ConsolePolicy {
    fn default() -> Self {
        Self {
            require_app_auth: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleAccessRequest {
    pub provider: AuthProvider,
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsPolicy {
    pub enforce_slo_targets: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeOpsPolicy {
    pub replica_count: u16,
    pub metrics: MetricsPolicy,
}

impl Default for RuntimeOpsPolicy {
    fn default() -> Self {
        Self {
            replica_count: 1,
            metrics: MetricsPolicy {
                enforce_slo_targets: false,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseMetadata {
    pub targets: Vec<String>,
    pub support_matrix: String,
}

pub fn validate_bigquery_naming(naming: &BigQueryNaming) -> Result<(), DecisionPolicyError> {
    if naming.dataset.trim().is_empty() {
        return Err(DecisionPolicyError::EmptyBigQueryDataset);
    }
    if naming.table.trim().is_empty() {
        return Err(DecisionPolicyError::EmptyBigQueryTable);
    }

    for value in [&naming.dataset, &naming.table] {
        if !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
        {
            return Err(DecisionPolicyError::InvalidBigQueryName(value.clone()));
        }
    }

    Ok(())
}

pub fn load_trusted_mobkit_modules_from_toml(
    toml_text: &str,
) -> Result<Vec<ModuleConfig>, DecisionPolicyError> {
    let parsed: TrustedMobkitToml =
        toml::from_str(toml_text).map_err(|err| DecisionPolicyError::TomlParse(err.to_string()))?;

    parsed
        .modules
        .into_iter()
        .map(|module| {
            if module.id.trim().is_empty() {
                return Err(DecisionPolicyError::MissingModuleId);
            }
            if module.command.trim().is_empty() {
                return Err(DecisionPolicyError::MissingModuleCommand);
            }
            Ok(ModuleConfig {
                id: module.id,
                command: module.command,
                args: module.args,
                restart_policy: module.restart_policy.unwrap_or(RestartPolicy::OnFailure),
            })
        })
        .collect()
}

pub fn enforce_console_route_access(
    auth_policy: &AuthPolicy,
    console_policy: &ConsolePolicy,
    request: &ConsoleAccessRequest,
) -> Result<(), DecisionPolicyError> {
    if !console_policy.require_app_auth {
        return Ok(());
    }

    if request.provider == AuthProvider::ServiceIdentity {
        if !request.email.starts_with("svc:") || request.email.len() <= 4 {
            return Err(DecisionPolicyError::InvalidServiceIdentity);
        }
        if !auth_policy
            .email_allowlist
            .iter()
            .any(|principal| principal == &request.email)
        {
            return Err(DecisionPolicyError::ServiceIdentityNotAllowlisted);
        }
        return Ok(());
    }

    if request.provider != auth_policy.default_provider {
        return Err(DecisionPolicyError::AuthProviderMismatch);
    }

    if matches!(request.provider, AuthProvider::TestProvider) {
        return Err(DecisionPolicyError::AuthProviderNotSupported);
    }

    if !auth_policy
        .email_allowlist
        .iter()
        .any(|email| email == &request.email)
    {
        return Err(DecisionPolicyError::EmailNotAllowlisted);
    }

    Ok(())
}

pub fn validate_runtime_ops_policy(policy: &RuntimeOpsPolicy) -> Result<(), DecisionPolicyError> {
    if policy.replica_count != 1 {
        return Err(DecisionPolicyError::ReplicaCountMustBeOne(
            policy.replica_count,
        ));
    }
    if policy.metrics.enforce_slo_targets {
        return Err(DecisionPolicyError::SloTargetsNotSupportedV01);
    }
    Ok(())
}

pub fn parse_release_metadata_json(
    json_text: &str,
) -> Result<ReleaseMetadata, DecisionPolicyError> {
    serde_json::from_str(json_text).map_err(|err| DecisionPolicyError::TomlParse(err.to_string()))
}

pub fn validate_release_metadata(metadata: &ReleaseMetadata) -> Result<(), DecisionPolicyError> {
    let mut seen = BTreeSet::new();
    for target in &metadata.targets {
        if !seen.insert(target.clone()) {
            return Err(DecisionPolicyError::DuplicateReleaseTarget(target.clone()));
        }
    }

    for required in REQUIRED_RELEASE_TARGETS {
        if !seen.contains(*required) {
            return Err(DecisionPolicyError::MissingReleaseTarget(
                (*required).to_string(),
            ));
        }
    }

    if metadata.support_matrix != "same-as-meerkat" {
        return Err(DecisionPolicyError::InvalidSupportMatrix(
            metadata.support_matrix.clone(),
        ));
    }

    Ok(())
}
