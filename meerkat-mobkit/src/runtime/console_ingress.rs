//! Console ingress types and JSON request/response structures.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRestJsonRequest {
    pub method: String,
    pub path: String,
    pub auth: Option<ConsoleAccessRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRestJsonResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleLiveSnapshot {
    pub running: bool,
    pub loaded_modules: Vec<String>,
}

impl ConsoleLiveSnapshot {
    pub fn new(running: bool, loaded_modules: Vec<String>) -> Self {
        let mut seen = BTreeSet::new();
        let mut deduped_modules = Vec::new();
        for module_id in loaded_modules {
            if seen.insert(module_id.clone()) {
                deduped_modules.push(module_id);
            }
        }
        Self {
            running,
            loaded_modules: deduped_modules,
        }
    }
}

pub fn handle_console_rest_json_route(
    decisions: &RuntimeDecisionState,
    request: &ConsoleRestJsonRequest,
) -> ConsoleRestJsonResponse {
    handle_console_rest_json_route_with_snapshot(decisions, request, None)
}

pub fn handle_console_rest_json_route_with_snapshot(
    decisions: &RuntimeDecisionState,
    request: &ConsoleRestJsonRequest,
    live_snapshot: Option<&ConsoleLiveSnapshot>,
) -> ConsoleRestJsonResponse {
    let (base_path, query_params) = split_path_and_query(&request.path);
    if request.method != "GET"
        || (base_path != CONSOLE_MODULES_ROUTE && base_path != CONSOLE_EXPERIENCE_ROUTE)
    {
        return ConsoleRestJsonResponse {
            status: 404,
            body: serde_json::json!({"error":"not_found"}),
        };
    }

    let resolved_auth = match resolve_console_auth(decisions, request.auth.as_ref(), &query_params)
    {
        Ok(auth) => auth,
        Err(error) => {
            return ConsoleRestJsonResponse {
                status: 401,
                body: serde_json::json!({
                    "error":"unauthorized",
                    "reason": console_auth_error_reason(&error),
                }),
            };
        }
    };

    match resolved_auth {
        Some(auth) => {
            if let Err(error) =
                enforce_console_route_access(&decisions.auth, &decisions.console, &auth)
            {
                return ConsoleRestJsonResponse {
                    status: 401,
                    body: serde_json::json!({
                        "error":"unauthorized",
                        "reason": auth_error_reason(&error),
                    }),
                };
            }
        }
        None if decisions.console.require_app_auth => {
            return ConsoleRestJsonResponse {
                status: 401,
                body: serde_json::json!({
                    "error":"unauthorized",
                    "reason":"missing_credentials",
                }),
            };
        }
        None => {}
    }

    let modules: Vec<String> = decisions
        .modules
        .iter()
        .map(|module| module.id.clone())
        .collect();
    let live_snapshot = live_snapshot
        .cloned()
        .unwrap_or_else(|| default_console_live_snapshot(decisions));
    let body = if base_path == CONSOLE_EXPERIENCE_ROUTE {
        build_console_experience_contract(&modules, &live_snapshot)
    } else {
        serde_json::json!({
            "contract_version": "0.1.0",
            "modules": modules
        })
    };
    ConsoleRestJsonResponse { status: 200, body }
}

fn default_console_live_snapshot(decisions: &RuntimeDecisionState) -> ConsoleLiveSnapshot {
    ConsoleLiveSnapshot::new(
        !decisions.modules.is_empty(),
        decisions
            .modules
            .iter()
            .map(|module| module.id.clone())
            .collect(),
    )
}

fn build_console_experience_contract(
    modules: &[String],
    live_snapshot: &ConsoleLiveSnapshot,
) -> Value {
    let module_panels = modules
        .iter()
        .map(|module_id| {
            serde_json::json!({
                "panel_id": format!("module.{module_id}"),
                "module_id": module_id,
                "title": format!("{module_id} module"),
                "route": format!("/console/modules/{module_id}"),
                "capabilities": {
                    "can_render": true,
                    "can_subscribe_activity": true,
                }
            })
        })
        .collect::<Vec<_>>();
    let sidebar_agents = live_snapshot
        .loaded_modules
        .iter()
        .map(|module_id| {
            serde_json::json!({
                "agent_id": module_id,
                "member_id": module_id,
                "label": module_id,
                "kind": "module_agent",
            })
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "contract_version": "0.1.0",
        "base_panel": {
            "panel_id": "console.home",
            "title": "Mob Console",
            "route": CONSOLE_EXPERIENCE_ROUTE,
            "capabilities": {
                "can_render": true,
                "surface": "console",
            }
        },
        "module_panels": module_panels,
        "agent_sidebar": {
            "panel_id": "console.agent_sidebar",
            "title": "Agents",
            "source_method": "mobkit/status",
            "refresh_policy": {
                "mode": "pull",
                "poll_interval_ms": 5000,
            },
            "selection_contract": {
                "selected_agent_id_field": "agent_id",
                "selected_member_id_field": "member_id",
                "emits_scope": "agent",
                "supported_scopes": ["mob", "agent"],
            },
            "list_item_contract": {
                "fields": ["agent_id", "member_id", "label", "kind"],
                "agent_id_field": "agent_id",
                "member_id_field": "member_id",
            },
            "live_snapshot": {
                "agents": sidebar_agents,
            }
        },
        "activity_feed": {
            "panel_id": "console.activity_feed",
            "title": "Activity",
            "transport": "sse",
            "source_method": EVENTS_SUBSCRIBE_METHOD,
            "supported_scopes": ["mob", "agent", "interaction"],
            "default_scope": "mob",
            "request_contract": {
                "scope": "mob|agent|interaction",
                "agent_id": "required when scope=agent",
                "last_event_id": "optional checkpoint from prior event_id",
            },
            "event_contract": {
                "envelope_fields": ["event_id", "source", "timestamp_ms", "event"],
                "event_type_path": "event.event_type",
                "frame_format": "id: <event_id>\\nevent: <event_type>\\ndata: <event_json>\\n\\n",
            },
            "keep_alive": {
                "interval_ms": SSE_KEEP_ALIVE_INTERVAL_MS,
                "event": SSE_KEEP_ALIVE_EVENT_NAME,
                "comment_frame": SSE_KEEP_ALIVE_COMMENT_FRAME,
            }
        },
        "chat_inspector": {
            "panel_id": "console.chat_inspector",
            "title": "Chat Inspector",
            "send_method": "mobkit/send_message",
            "observe_route": "/interactions/stream",
            "transport": "rpc+sse",
            "request_contract": {
                "member_id": "required target member id",
                "message": "required user text to send",
            },
            "response_contract": {
                "accepted": "boolean request acceptance flag",
                "member_id": "echoed target member id",
                "session_id": "accepting Meerkat session id for correlation",
            },
            "event_contract": {
                "agent_event_type_path": "type",
            }
        },
        "topology": {
            "panel_id": "console.topology",
            "title": "Topology",
            "source_method": "mobkit/status",
            "route_method": "mobkit/routing/routes/list",
            "refresh_policy": {
                "mode": "pull",
                "poll_interval_ms": 5000,
            },
            "graph_contract": {
                "node_id_field": "module_id",
                "edge_fields": ["from", "to", "route"],
            },
            "live_snapshot": {
                "nodes": &live_snapshot.loaded_modules,
                "node_count": live_snapshot.loaded_modules.len(),
            }
        },
        "health_overview": {
            "panel_id": "console.health_overview",
            "title": "Health",
            "source_method": "mobkit/status",
            "activity_source_method": EVENTS_SUBSCRIBE_METHOD,
            "refresh_policy": {
                "mode": "pull_and_stream",
                "poll_interval_ms": 5000,
            },
            "status_contract": {
                "running_field": "running",
                "loaded_modules_field": "loaded_modules",
            },
            "live_snapshot": {
                "running": live_snapshot.running,
                "loaded_modules": &live_snapshot.loaded_modules,
                "loaded_module_count": live_snapshot.loaded_modules.len(),
            }
        }
    })
}

fn split_path_and_query(path: &str) -> (&str, BTreeMap<String, String>) {
    let (base, query) = path.split_once('?').unwrap_or((path, ""));
    let mut params = BTreeMap::new();
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = part.split_once('=').unwrap_or((part, ""));
        if !k.is_empty() {
            params.insert(k.to_string(), v.to_string());
        }
    }
    (base, params)
}

fn resolve_console_auth(
    decisions: &RuntimeDecisionState,
    explicit_auth: Option<&ConsoleAccessRequest>,
    query_params: &BTreeMap<String, String>,
) -> Result<Option<ConsoleAccessRequest>, ConsoleAuthResolutionError> {
    if let Some(auth) = explicit_auth {
        return Ok(Some(auth.clone()));
    }

    if !decisions.console.require_app_auth {
        return Ok(None);
    }

    // Check query-param auth_token (also used as the bearer-header injection
    // point by the HTTP handler — see console_json_handler).
    match query_params.get("auth_token") {
        Some(token) => resolve_console_auth_from_token(decisions, token).map(Some),
        None => Ok(None),
    }
}

/// Extract a bearer token from an `Authorization: Bearer <token>` header value.
pub fn extract_bearer_token_from_header(header_value: &str) -> Option<&str> {
    let token = header_value.strip_prefix("Bearer ")?;
    if token.is_empty() { None } else { Some(token) }
}

fn resolve_console_auth_from_token(
    decisions: &RuntimeDecisionState,
    token: &str,
) -> Result<ConsoleAccessRequest, ConsoleAuthResolutionError> {
    if decisions.trusted_oidc.audience.trim().is_empty() {
        return Err(ConsoleAuthResolutionError::InvalidTrustedOidcConfig);
    }

    let discovery = parse_oidc_discovery_json(&decisions.trusted_oidc.discovery_json)
        .map_err(|_| ConsoleAuthResolutionError::InvalidTrustedOidcConfig)?;
    let jwks = parse_jwks_json(&decisions.trusted_oidc.jwks_json)
        .map_err(|_| ConsoleAuthResolutionError::InvalidTrustedOidcConfig)?;
    let header =
        inspect_jwt_header(token).map_err(|_| ConsoleAuthResolutionError::InvalidTokenHeader)?;

    if header.alg == "HS256"
        && !hs256_allowed_for_development_issuer(&discovery.issuer, &discovery.jwks_uri)
    {
        return Err(ConsoleAuthResolutionError::Hs256NotAllowed);
    }

    let key = select_jwk_for_token(&jwks, header.kid.as_deref(), &header.alg)
        .map_err(|_| ConsoleAuthResolutionError::JwksKeyNotFound)?;
    let verification_key = build_jwt_verification_key(key, &header.alg)
        .map_err(|_| ConsoleAuthResolutionError::InvalidJwksKeyMaterial)?;

    let now_epoch_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let claims = validate_jwt_with_verification_key(
        token,
        &verification_key,
        &JwtClaimsValidationConfig {
            issuer: Some(discovery.issuer),
            audience: Some(decisions.trusted_oidc.audience.clone()),
            now_epoch_seconds,
            leeway_seconds: 30,
        },
    )
    .map_err(|_| ConsoleAuthResolutionError::InvalidToken)?;

    let principal = claims
        .email
        .or(claims.subject)
        .ok_or(ConsoleAuthResolutionError::MissingTokenIdentity)?;
    let provider =
        if claims.actor_type.as_deref() == Some("service") || principal.starts_with("svc:") {
            AuthProvider::ServiceIdentity
        } else {
            match claims.provider.as_deref() {
                Some("google_oauth") => AuthProvider::GoogleOAuth,
                Some("github_oauth") => AuthProvider::GitHubOAuth,
                Some("generic_oidc") => AuthProvider::GenericOidc,
                _ => AuthProvider::GenericOidc,
            }
        };

    Ok(ConsoleAccessRequest {
        provider,
        email: principal,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConsoleAuthResolutionError {
    InvalidTrustedOidcConfig,
    InvalidTokenHeader,
    JwksKeyNotFound,
    InvalidJwksKeyMaterial,
    InvalidToken,
    MissingTokenIdentity,
    Hs256NotAllowed,
}

fn console_auth_error_reason(error: &ConsoleAuthResolutionError) -> &'static str {
    match error {
        ConsoleAuthResolutionError::InvalidTrustedOidcConfig => "invalid_trusted_oidc_config",
        ConsoleAuthResolutionError::InvalidTokenHeader => "invalid_token_header",
        ConsoleAuthResolutionError::JwksKeyNotFound => "jwks_key_not_found",
        ConsoleAuthResolutionError::InvalidJwksKeyMaterial => "invalid_jwks_key_material",
        ConsoleAuthResolutionError::InvalidToken => "invalid_token",
        ConsoleAuthResolutionError::MissingTokenIdentity => "missing_token_identity",
        ConsoleAuthResolutionError::Hs256NotAllowed => "hs256_not_allowed",
    }
}

fn hs256_allowed_for_development_issuer(issuer: &str, jwks_uri: &str) -> bool {
    match (extract_uri_host(issuer), extract_uri_host(jwks_uri)) {
        (Some(issuer_host), Some(jwks_host)) => {
            is_development_host(issuer_host) && is_development_host(jwks_host)
        }
        _ => false,
    }
}

fn extract_uri_host(uri: &str) -> Option<&str> {
    let after_scheme = uri.split_once("://").map_or(uri, |(_, rest)| rest);
    let authority_with_path = after_scheme.split('/').next()?;
    let authority = authority_with_path
        .rsplit('@')
        .next()
        .unwrap_or(authority_with_path);
    if authority.is_empty() {
        return None;
    }

    if let Some(stripped) = authority.strip_prefix('[') {
        let (ipv6_host, _) = stripped.split_once(']')?;
        return if ipv6_host.is_empty() {
            None
        } else {
            Some(ipv6_host)
        };
    }

    let host = authority
        .split_once(':')
        .map_or(authority, |(hostname, _)| hostname);
    if host.is_empty() { None } else { Some(host) }
}

fn is_development_host(host: &str) -> bool {
    let lowercase = host.to_ascii_lowercase();
    lowercase == "localhost"
        || lowercase == "127.0.0.1"
        || lowercase == "::1"
        || lowercase.ends_with(".localhost")
}

fn auth_error_reason(error: &DecisionPolicyError) -> &'static str {
    match error {
        DecisionPolicyError::AuthProviderMismatch => "provider_mismatch",
        DecisionPolicyError::AuthProviderNotSupported => "provider_not_supported",
        DecisionPolicyError::EmailNotAllowlisted => "email_not_allowlisted",
        DecisionPolicyError::InvalidServiceIdentity => "invalid_service_identity",
        DecisionPolicyError::ServiceIdentityNotAllowlisted => "service_identity_not_allowlisted",
        _ => "policy_denied",
    }
}
