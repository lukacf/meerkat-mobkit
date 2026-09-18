//! Explicit server-side public OpenAI Live registration shared by gateways.
//! Parsing configuration does not establish authenticated readiness.

use meerkat_core::{AuthBindingRef, BindingId, BindingOrigin, ProfileId, RealmId};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicLiveRegistration {
    pub principal: String,
    pub realm: RealmId,
    pub binding: AuthBindingRef,
    pub voice: String,
    pub session_instructions: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrationWire {
    principal: String,
    realm: String,
    auth_binding: BindingWire,
    voice: String,
    session_instructions: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingWire {
    realm: String,
    binding: String,
    profile: Option<String>,
}

fn nonempty(value: &str, field: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(format!("{field} must be a non-empty string"))
    } else {
        Ok(value.to_string())
    }
}

impl PublicLiveRegistration {
    /// Accept only a configured binding reference, never a browser-provided
    /// credential, model override, experimental profile, or provider.
    pub fn parse(value: &Value) -> Result<Self, String> {
        let wire: RegistrationWire =
            serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
        let principal = nonempty(&wire.principal, "principal")?;
        let realm = RealmId::parse(nonempty(&wire.realm, "realm")?)
            .map_err(|error| format!("realm is invalid: {error}"))?;
        let binding_realm =
            RealmId::parse(nonempty(&wire.auth_binding.realm, "auth_binding.realm")?)
                .map_err(|error| format!("auth_binding.realm is invalid: {error}"))?;
        if binding_realm != realm {
            return Err("auth_binding.realm must equal openai_live.realm".to_string());
        }
        let binding = BindingId::parse(nonempty(
            &wire.auth_binding.binding,
            "auth_binding.binding",
        )?)
        .map_err(|error| format!("auth_binding.binding is invalid: {error}"))?;
        let profile = wire
            .auth_binding
            .profile
            .map(|profile| {
                ProfileId::parse(nonempty(&profile, "auth_binding.profile")?)
                    .map_err(|error| format!("auth_binding.profile is invalid: {error}"))
            })
            .transpose()?;
        Ok(Self {
            principal,
            realm,
            binding: AuthBindingRef {
                realm: binding_realm,
                binding,
                profile,
                origin: BindingOrigin::Configured,
            },
            voice: nonempty(&wire.voice, "voice")?,
            session_instructions: wire
                .session_instructions
                .map(|instructions| nonempty(&instructions, "session_instructions"))
                .transpose()?,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn registration() -> Value {
        json!({
            "principal": "alice",
            "realm": "voice",
            "auth_binding": {"realm": "voice", "binding": "openai"},
            "voice": "marin"
        })
    }

    #[test]
    fn public_live_registration_preserves_exact_configured_binding() {
        let parsed = PublicLiveRegistration::parse(&registration()).expect("registration");
        assert_eq!(parsed.principal, "alice");
        assert_eq!(parsed.binding.origin, BindingOrigin::Configured);
        assert_eq!(parsed.binding.realm, parsed.realm);
        assert_eq!(parsed.binding.binding.as_str(), "openai");
        assert!(parsed.binding.profile.is_none());
    }

    #[test]
    fn public_live_registration_rejects_secrets_overrides_and_cross_realm_binding() {
        for (field, value) in [
            ("api_key", json!("not-a-secret-fixture")),
            ("provider", json!("openai")),
            ("model", json!("gpt-live-1")),
            ("voice", json!(" ")),
            ("principal", json!("")),
        ] {
            let mut invalid = registration();
            invalid[field] = value;
            assert!(PublicLiveRegistration::parse(&invalid).is_err());
        }
        let mut wrong_realm = registration();
        wrong_realm["auth_binding"]["realm"] = json!("elsewhere");
        assert!(PublicLiveRegistration::parse(&wrong_realm).is_err());
        let mut secret = registration();
        secret["auth_binding"]["api_key"] = json!("not-a-secret-fixture");
        assert!(PublicLiveRegistration::parse(&secret).is_err());
    }
}
