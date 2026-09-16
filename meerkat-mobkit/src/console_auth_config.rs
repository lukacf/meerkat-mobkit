//! Shared explicit JWT console authentication configuration for both gateways.

use base64::Engine as _;
use serde_json::{Value, json};

use crate::{AuthPolicy, AuthProvider, BigQueryNaming, ConsolePolicy, RuntimeDecisionState};

pub fn parse_console_auth_config(value: &Value) -> Result<RuntimeDecisionState, String> {
    let object = value
        .as_object()
        .ok_or("auth_config must be a JSON object")?;
    let provider = object
        .get("provider")
        .and_then(Value::as_str)
        .or_else(|| {
            (object.contains_key("shared_secret") || object.contains_key("sharedSecret"))
                .then_some("jwt")
        })
        .ok_or("auth_config.provider is required")?;
    if provider != "jwt" {
        return Err(format!("unsupported auth_config.provider '{provider}'"));
    }
    let secret = object
        .get("shared_secret")
        .or_else(|| object.get("sharedSecret"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or("auth_config.shared_secret must be a non-empty string")?;
    let issuer = object
        .get("issuer")
        .and_then(Value::as_str)
        .unwrap_or("http://127.0.0.1/mobkit-gateway");
    let audience = object
        .get("audience")
        .and_then(Value::as_str)
        .unwrap_or("persistent-gateway");
    let allowlist = object
        .get("email_allowlist")
        .or_else(|| object.get("emailAllowlist"))
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    let mut decisions = RuntimeDecisionState::local_console(
        ConsolePolicy {
            require_app_auth: true,
            ..ConsolePolicy::default()
        },
        Some(BigQueryNaming {
            dataset: "default_dataset".to_string(),
            table: "default_table".to_string(),
        }),
    );
    decisions.auth = AuthPolicy {
        default_provider: AuthProvider::GenericOidc,
        email_allowlist: allowlist,
    };
    decisions.trusted_oidc = crate::TrustedOidcRuntimeConfig {
        discovery_json:
            json!({"issuer":issuer,"jwks_uri":"http://127.0.0.1/mobkit-gateway/jwks.json"})
                .to_string(),
        jwks_json: json!({"keys":[{"kty":"oct","alg":"HS256",
            "k":base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(secret.as_bytes())}]})
        .to_string(),
        audience: audience.to_string(),
    };
    Ok(decisions)
}
