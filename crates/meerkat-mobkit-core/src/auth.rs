use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtValidationConfig {
    pub shared_secret: String,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub now_epoch_seconds: u64,
    pub leeway_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwtValidationError {
    InvalidFormat,
    InvalidBase64,
    InvalidJson,
    UnsupportedAlgorithm(String),
    InvalidSignature,
    Expired,
    NotYetValid,
    IssuerMismatch,
    AudienceMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatedJwt {
    pub subject: Option<String>,
    pub email: Option<String>,
    pub provider: Option<String>,
    pub actor_type: Option<String>,
    pub issuer: Option<String>,
    pub audience: Vec<String>,
    pub expires_at_epoch_seconds: Option<u64>,
    pub not_before_epoch_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct JwtHeader {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OidcDiscoveryDocument {
    pub issuer: String,
    pub jwks_uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwksDocument {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jwk {
    pub kid: Option<String>,
    pub kty: String,
    #[serde(default)]
    pub alg: Option<String>,
    #[serde(default)]
    pub k: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OidcContractError {
    InvalidJson,
    MissingIssuer,
    MissingJwksUri,
    MissingKeys,
    NoMatchingKey,
    UnsupportedKeyType(String),
    MissingSymmetricKeyMaterial,
    InvalidKeyEncoding,
    InvalidSymmetricKeyMaterial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtHeaderView {
    pub alg: String,
    pub kid: Option<String>,
}

pub fn validate_jwt_locally(
    token: &str,
    config: &JwtValidationConfig,
) -> Result<ValidatedJwt, JwtValidationError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(JwtValidationError::InvalidFormat);
    }

    let header_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| JwtValidationError::InvalidBase64)?;
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| JwtValidationError::InvalidBase64)?;
    let signature = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| JwtValidationError::InvalidBase64)?;

    let header: JwtHeader =
        serde_json::from_slice(&header_bytes).map_err(|_| JwtValidationError::InvalidJson)?;
    if header.alg != "HS256" {
        return Err(JwtValidationError::UnsupportedAlgorithm(header.alg));
    }

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let mut mac = HmacSha256::new_from_slice(config.shared_secret.as_bytes())
        .map_err(|_| JwtValidationError::InvalidSignature)?;
    mac.update(signing_input.as_bytes());
    let expected = mac.finalize().into_bytes();
    if expected.as_slice() != signature.as_slice() {
        return Err(JwtValidationError::InvalidSignature);
    }

    let claims: Value =
        serde_json::from_slice(&payload_bytes).map_err(|_| JwtValidationError::InvalidJson)?;

    let exp = claims.get("exp").and_then(Value::as_u64);
    let nbf = claims.get("nbf").and_then(Value::as_u64);
    let iss = claims
        .get("iss")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let aud = extract_audiences(&claims);

    if let Some(exp) = exp {
        let threshold = config
            .now_epoch_seconds
            .saturating_sub(config.leeway_seconds);
        if exp < threshold {
            return Err(JwtValidationError::Expired);
        }
    }
    if let Some(nbf) = nbf {
        let threshold = config
            .now_epoch_seconds
            .saturating_add(config.leeway_seconds);
        if nbf > threshold {
            return Err(JwtValidationError::NotYetValid);
        }
    }

    if let Some(expected_issuer) = &config.issuer {
        if iss.as_deref() != Some(expected_issuer.as_str()) {
            return Err(JwtValidationError::IssuerMismatch);
        }
    }
    if let Some(expected_audience) = &config.audience {
        if !aud.iter().any(|entry| entry == expected_audience) {
            return Err(JwtValidationError::AudienceMismatch);
        }
    }

    Ok(ValidatedJwt {
        subject: claims
            .get("sub")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        email: claims
            .get("email")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        provider: claims
            .get("provider")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        actor_type: claims
            .get("actor_type")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        issuer: iss,
        audience: aud,
        expires_at_epoch_seconds: exp,
        not_before_epoch_seconds: nbf,
    })
}

pub fn inspect_jwt_header(token: &str) -> Result<JwtHeaderView, JwtValidationError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(JwtValidationError::InvalidFormat);
    }
    let header_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| JwtValidationError::InvalidBase64)?;
    let header: JwtHeader =
        serde_json::from_slice(&header_bytes).map_err(|_| JwtValidationError::InvalidJson)?;
    Ok(JwtHeaderView {
        alg: header.alg,
        kid: header.kid,
    })
}

pub fn parse_oidc_discovery_json(
    json_text: &str,
) -> Result<OidcDiscoveryDocument, OidcContractError> {
    let doc: OidcDiscoveryDocument =
        serde_json::from_str(json_text).map_err(|_| OidcContractError::InvalidJson)?;
    if doc.issuer.trim().is_empty() {
        return Err(OidcContractError::MissingIssuer);
    }
    if doc.jwks_uri.trim().is_empty() {
        return Err(OidcContractError::MissingJwksUri);
    }
    Ok(doc)
}

pub fn parse_jwks_json(json_text: &str) -> Result<JwksDocument, OidcContractError> {
    let doc: JwksDocument =
        serde_json::from_str(json_text).map_err(|_| OidcContractError::InvalidJson)?;
    if doc.keys.is_empty() {
        return Err(OidcContractError::MissingKeys);
    }
    Ok(doc)
}

pub fn select_jwk_for_token<'a>(
    jwks: &'a JwksDocument,
    kid: Option<&str>,
    alg: &str,
) -> Result<&'a Jwk, OidcContractError> {
    if let Some(kid) = kid {
        return jwks
            .keys
            .iter()
            .find(|key| {
                key.kid.as_deref() == Some(kid)
                    && key.alg.as_deref().is_none_or(|key_alg| key_alg == alg)
            })
            .ok_or(OidcContractError::NoMatchingKey);
    }

    jwks.keys
        .iter()
        .find(|key| key.alg.as_deref().is_none_or(|key_alg| key_alg == alg))
        .ok_or(OidcContractError::NoMatchingKey)
}

pub fn extract_hs256_shared_secret(key: &Jwk) -> Result<String, OidcContractError> {
    if key.kty != "oct" {
        return Err(OidcContractError::UnsupportedKeyType(key.kty.clone()));
    }
    let encoded = key
        .k
        .as_ref()
        .ok_or(OidcContractError::MissingSymmetricKeyMaterial)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| OidcContractError::InvalidKeyEncoding)?;
    String::from_utf8(bytes).map_err(|_| OidcContractError::InvalidSymmetricKeyMaterial)
}

fn extract_audiences(claims: &Value) -> Vec<String> {
    match claims.get("aud") {
        Some(Value::String(aud)) => vec![aud.clone()],
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}
