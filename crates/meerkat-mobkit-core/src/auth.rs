use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use ring::signature::{self, RsaPublicKeyComponents, UnparsedPublicKey};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JwtClaimsValidationConfig {
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub now_epoch_seconds: u64,
    pub leeway_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JwtVerificationKey {
    Hs256(Vec<u8>),
    Rs256 { modulus: Vec<u8>, exponent: Vec<u8> },
    Es256P256 { public_key: Vec<u8> },
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
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
    #[serde(default)]
    pub crv: Option<String>,
    #[serde(default)]
    pub x: Option<String>,
    #[serde(default)]
    pub y: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OidcContractError {
    InvalidJson,
    MissingIssuer,
    MissingJwksUri,
    MissingKeys,
    NoMatchingKey,
    UnsupportedKeyType(String),
    UnsupportedJwtAlgorithm(String),
    MissingSymmetricKeyMaterial,
    MissingRsaKeyMaterial,
    MissingEcKeyMaterial,
    InvalidKeyEncoding,
    InvalidSymmetricKeyMaterial,
    InvalidRsaKeyMaterial,
    InvalidEcKeyMaterial,
    UnsupportedEllipticCurve(String),
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
    let claims_config = JwtClaimsValidationConfig {
        issuer: config.issuer.clone(),
        audience: config.audience.clone(),
        now_epoch_seconds: config.now_epoch_seconds,
        leeway_seconds: config.leeway_seconds,
    };
    let key = JwtVerificationKey::Hs256(config.shared_secret.as_bytes().to_vec());
    validate_jwt_with_verification_key(token, &key, &claims_config)
}

pub fn validate_jwt_with_verification_key(
    token: &str,
    verification_key: &JwtVerificationKey,
    config: &JwtClaimsValidationConfig,
) -> Result<ValidatedJwt, JwtValidationError> {
    let parsed = parse_jwt(token)?;
    if !key_supports_algorithm(verification_key, parsed.header.alg.as_str()) {
        return Err(JwtValidationError::UnsupportedAlgorithm(parsed.header.alg));
    }
    verify_signature(
        verification_key,
        parsed.header.alg.as_str(),
        parsed.signing_input.as_bytes(),
        &parsed.signature,
    )?;
    validate_claims(&parsed.claims, config)
}

pub fn build_jwt_verification_key(
    key: &Jwk,
    alg: &str,
) -> Result<JwtVerificationKey, OidcContractError> {
    match alg {
        "HS256" => build_hs256_key(key),
        "RS256" => build_rs256_key(key),
        "ES256" => build_es256_key(key),
        unsupported => Err(OidcContractError::UnsupportedJwtAlgorithm(
            unsupported.to_string(),
        )),
    }
}

fn build_hs256_key(key: &Jwk) -> Result<JwtVerificationKey, OidcContractError> {
    if key.kty != "oct" {
        return Err(OidcContractError::UnsupportedKeyType(key.kty.clone()));
    }
    let encoded = key
        .k
        .as_deref()
        .ok_or(OidcContractError::MissingSymmetricKeyMaterial)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| OidcContractError::InvalidKeyEncoding)?;
    Ok(JwtVerificationKey::Hs256(bytes))
}

fn build_rs256_key(key: &Jwk) -> Result<JwtVerificationKey, OidcContractError> {
    if key.kty != "RSA" {
        return Err(OidcContractError::UnsupportedKeyType(key.kty.clone()));
    }
    let modulus = key
        .n
        .as_deref()
        .ok_or(OidcContractError::MissingRsaKeyMaterial)
        .and_then(decode_key_component)?;
    let exponent = key
        .e
        .as_deref()
        .ok_or(OidcContractError::MissingRsaKeyMaterial)
        .and_then(decode_key_component)?;
    if modulus.is_empty() || exponent.is_empty() {
        return Err(OidcContractError::InvalidRsaKeyMaterial);
    }
    Ok(JwtVerificationKey::Rs256 { modulus, exponent })
}

fn build_es256_key(key: &Jwk) -> Result<JwtVerificationKey, OidcContractError> {
    if key.kty != "EC" {
        return Err(OidcContractError::UnsupportedKeyType(key.kty.clone()));
    }
    let curve = key
        .crv
        .as_deref()
        .ok_or(OidcContractError::MissingEcKeyMaterial)?;
    if curve != "P-256" {
        return Err(OidcContractError::UnsupportedEllipticCurve(
            curve.to_string(),
        ));
    }

    let x = key
        .x
        .as_deref()
        .ok_or(OidcContractError::MissingEcKeyMaterial)
        .and_then(decode_key_component)?;
    let y = key
        .y
        .as_deref()
        .ok_or(OidcContractError::MissingEcKeyMaterial)
        .and_then(decode_key_component)?;
    if x.len() != 32 || y.len() != 32 {
        return Err(OidcContractError::InvalidEcKeyMaterial);
    }

    let mut public_key = Vec::with_capacity(65);
    public_key.push(0x04);
    public_key.extend_from_slice(&x);
    public_key.extend_from_slice(&y);
    Ok(JwtVerificationKey::Es256P256 { public_key })
}

fn decode_key_component(encoded: &str) -> Result<Vec<u8>, OidcContractError> {
    URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| OidcContractError::InvalidKeyEncoding)
}

struct ParsedJwt {
    header: JwtHeader,
    claims: Value,
    signing_input: String,
    signature: Vec<u8>,
}

fn parse_jwt(token: &str) -> Result<ParsedJwt, JwtValidationError> {
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
    let claims: Value =
        serde_json::from_slice(&payload_bytes).map_err(|_| JwtValidationError::InvalidJson)?;
    let signing_input = format!("{}.{}", parts[0], parts[1]);

    Ok(ParsedJwt {
        header,
        claims,
        signing_input,
        signature,
    })
}

fn key_supports_algorithm(key: &JwtVerificationKey, alg: &str) -> bool {
    matches!(
        (key, alg),
        (JwtVerificationKey::Hs256(_), "HS256")
            | (JwtVerificationKey::Rs256 { .. }, "RS256")
            | (JwtVerificationKey::Es256P256 { .. }, "ES256")
    )
}

fn verify_signature(
    verification_key: &JwtVerificationKey,
    alg: &str,
    signing_input: &[u8],
    signature: &[u8],
) -> Result<(), JwtValidationError> {
    match (verification_key, alg) {
        (JwtVerificationKey::Hs256(secret), "HS256") => {
            let mut mac = HmacSha256::new_from_slice(secret)
                .map_err(|_| JwtValidationError::InvalidSignature)?;
            mac.update(signing_input);
            let expected = mac.finalize().into_bytes();
            if expected.len() != signature.len()
                || expected
                    .iter()
                    .zip(signature.iter())
                    .any(|(left, right)| left != right)
            {
                return Err(JwtValidationError::InvalidSignature);
            }
            Ok(())
        }
        (JwtVerificationKey::Rs256 { modulus, exponent }, "RS256") => {
            let components = RsaPublicKeyComponents {
                n: modulus.as_slice(),
                e: exponent.as_slice(),
            };
            components
                .verify(
                    &signature::RSA_PKCS1_2048_8192_SHA256,
                    signing_input,
                    signature,
                )
                .map_err(|_| JwtValidationError::InvalidSignature)
        }
        (JwtVerificationKey::Es256P256 { public_key }, "ES256") => {
            UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, public_key.as_slice())
                .verify(signing_input, signature)
                .map_err(|_| JwtValidationError::InvalidSignature)
        }
        (_, unsupported) => Err(JwtValidationError::UnsupportedAlgorithm(
            unsupported.to_string(),
        )),
    }
}

fn validate_claims(
    claims: &Value,
    config: &JwtClaimsValidationConfig,
) -> Result<ValidatedJwt, JwtValidationError> {
    let exp = claims.get("exp").and_then(Value::as_u64);
    let nbf = claims.get("nbf").and_then(Value::as_u64);
    let iss = claims
        .get("iss")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let aud = extract_audiences(claims);

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
    let bytes = match build_hs256_key(key)? {
        JwtVerificationKey::Hs256(bytes) => bytes,
        _ => return Err(OidcContractError::InvalidSymmetricKeyMaterial),
    };
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
