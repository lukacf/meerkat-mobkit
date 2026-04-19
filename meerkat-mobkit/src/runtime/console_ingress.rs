//! Console ingress types and JSON request/response structures.

use super::*;
use crate::rpc::MOBKIT_CONTRACT_VERSION;

/// Console-facing view of a single mob member.
///
/// Narrow projection of meerkat's `MobMemberListEntry` enriched with the
/// current bridge session id (which meerkat doesn't carry on the entry).
/// Wire field names match the console JSON — `agent_identity`, `role`,
/// `state` — so the admin UI reads them unchanged from the live snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleMember {
    pub agent_identity: String,
    pub role: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub wired_to: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub labels: std::collections::BTreeMap<String, String>,
}

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
pub struct ConsoleAgentLiveSnapshot {
    pub agent_id: String,
    pub member_id: String,
    pub label: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watched: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "alertLevel"
    )]
    pub alert_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "degradedReason"
    )]
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleLiveSnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_id: Option<String>,
    pub running: bool,
    pub loaded_modules: Vec<String>,
    #[serde(default)]
    pub agents: Vec<ConsoleAgentLiveSnapshot>,
    pub members: Vec<ConsoleMember>,
    pub has_mob_runtime: bool,
}

impl ConsoleLiveSnapshot {
    pub fn new(
        runtime_id: Option<String>,
        running: bool,
        loaded_modules: Vec<String>,
        agents: Vec<ConsoleAgentLiveSnapshot>,
        members: Vec<ConsoleMember>,
        has_mob_runtime: bool,
    ) -> Self {
        let mut seen = BTreeSet::new();
        let mut deduped_modules = Vec::new();
        for module_id in loaded_modules {
            if seen.insert(module_id.clone()) {
                deduped_modules.push(module_id);
            }
        }
        let mut seen_agents = BTreeSet::new();
        let mut deduped_agents = Vec::new();
        for agent in agents {
            if seen_agents.insert(agent.agent_id.clone()) {
                deduped_agents.push(agent);
            }
        }
        Self {
            runtime_id,
            running,
            loaded_modules: deduped_modules,
            agents: deduped_agents,
            members,
            has_mob_runtime,
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
            "contract_version": MOBKIT_CONTRACT_VERSION,
            "modules": modules
        })
    };
    ConsoleRestJsonResponse { status: 200, body }
}

fn default_console_live_snapshot(decisions: &RuntimeDecisionState) -> ConsoleLiveSnapshot {
    let loaded_modules = decisions
        .modules
        .iter()
        .map(|module| module.id.clone())
        .collect::<Vec<_>>();
    let agents = loaded_modules
        .iter()
        .map(|module_id| ConsoleAgentLiveSnapshot {
            agent_id: module_id.clone(),
            member_id: module_id.clone(),
            label: module_id.clone(),
            kind: "module_agent".to_string(),
            identity: None,
            role: None,
            state: Some("idle".to_string()),
            session_id: None,
            response_phase: None,
            watched: None,
            alert_level: None,
            degraded: None,
            degraded_reason: None,
        })
        .collect::<Vec<_>>();
    ConsoleLiveSnapshot::new(
        None,
        !decisions.modules.is_empty(),
        loaded_modules,
        agents,
        Vec::new(),
        false,
    )
}

fn build_console_experience_contract(
    modules: &[String],
    live_snapshot: &ConsoleLiveSnapshot,
) -> Value {
    fn has_extended_agent_contract(agent: &ConsoleAgentLiveSnapshot) -> bool {
        agent.role.is_some()
            || agent.session_id.is_some()
            || agent.response_phase.is_some()
            || agent.watched.is_some()
            || agent.alert_level.is_some()
            || agent.degraded.is_some()
            || agent.degraded_reason.is_some()
            || agent.kind != "module_agent"
            || agent.member_id != agent.agent_id
            || agent.label != agent.agent_id
    }

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

    // P0 fix: Build sidebar from the full mob roster (members) when a mob
    // runtime is present, so multi-instance profiles (e.g. 5 profiles → 15
    // agents) enumerate every individual agent, not just profile-level IDs.
    // Fall back to loaded_modules for module-only runtimes.
    let sidebar_agents: Vec<Value> =
        if live_snapshot.has_mob_runtime && !live_snapshot.members.is_empty() {
            let mut sorted_members: Vec<&ConsoleMember> = live_snapshot.members.iter().collect();
            sorted_members.sort_by(|a, b| a.agent_identity.cmp(&b.agent_identity));
            sorted_members
                .iter()
                .map(|member| {
                    let label = member
                        .labels
                        .get("display_name")
                        .cloned()
                        .unwrap_or_else(|| member.agent_identity.clone());
                    let addressable = member
                        .labels
                        .get("addressable")
                        .map(|v| v != "false")
                        .unwrap_or(true);
                    let watched = member
                        .labels
                        .get("console_watched")
                        .map(|value| value == "true");
                    let alert_level = member
                        .labels
                        .get("console_alert_level")
                        .filter(|value| matches!(value.as_str(), "elevated" | "critical"))
                        .cloned();
                    let degraded = member
                        .labels
                        .get("console_degraded")
                        .map(|value| value == "true");
                    let degraded_reason = member.labels.get("console_degraded_reason").cloned();
                    let singleton = member
                        .labels
                        .get("singleton")
                        .map(|v| v == "true")
                        .unwrap_or(false);
                    let group = member
                        .labels
                        .get("group")
                        .cloned()
                        .unwrap_or_else(|| member.role.clone());
                    serde_json::json!({
                        "agent_id": member.agent_identity,
                        "member_id": member.agent_identity,
                        "identity": member.agent_identity,
                        "label": label,
                        "kind": "mob_agent",
                        "role": member.role,
                        "state": member.state,
                        "session_id": member.session_id,
                        "wired_to": member.wired_to,
                        "labels": member.labels,
                        "group": group,
                        "addressable": addressable,
                        "watched": watched,
                        "alertLevel": alert_level,
                        "degraded": degraded,
                        "degradedReason": degraded_reason,
                        "affordances": {
                            "addressable": addressable,
                            "can_send_message": addressable,
                            "can_retire": !singleton,
                            "can_respawn": true,
                            "runtime_mode": "mob_agent",
                        },
                    })
                })
                .collect()
        } else {
            live_snapshot
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
                .collect()
        };

    let sidebar_agents: Vec<Value> =
        if live_snapshot.has_mob_runtime && !live_snapshot.members.is_empty() {
            sidebar_agents
        } else if live_snapshot.agents.iter().any(has_extended_agent_contract) {
            live_snapshot
                .agents
                .iter()
                .map(|agent| {
                    let mut record = serde_json::Map::new();
                    record.insert(
                        "agent_id".to_string(),
                        Value::String(agent.agent_id.clone()),
                    );
                    record.insert(
                        "member_id".to_string(),
                        Value::String(agent.member_id.clone()),
                    );
                    record.insert("label".to_string(), Value::String(agent.label.clone()));
                    record.insert("kind".to_string(), Value::String(agent.kind.clone()));
                    if let Some(role) = &agent.role {
                        record.insert("role".to_string(), Value::String(role.clone()));
                    }
                    if let Some(state) = &agent.state {
                        record.insert("state".to_string(), Value::String(state.clone()));
                    }
                    if let Some(identity) = &agent.identity {
                        record.insert("identity".to_string(), Value::String(identity.clone()));
                    }
                    if let Some(session_id) = &agent.session_id {
                        record.insert("session_id".to_string(), Value::String(session_id.clone()));
                    }
                    if let Some(response_phase) = &agent.response_phase {
                        record.insert(
                            "response_phase".to_string(),
                            Value::String(response_phase.clone()),
                        );
                    }
                    if let Some(watched) = agent.watched {
                        record.insert("watched".to_string(), Value::Bool(watched));
                    }
                    if let Some(alert_level) = &agent.alert_level {
                        record.insert("alertLevel".to_string(), Value::String(alert_level.clone()));
                    }
                    if let Some(degraded) = agent.degraded {
                        record.insert("degraded".to_string(), Value::Bool(degraded));
                    }
                    if let Some(degraded_reason) = &agent.degraded_reason {
                        record.insert(
                            "degradedReason".to_string(),
                            Value::String(degraded_reason.clone()),
                        );
                    }
                    Value::Object(record)
                })
                .collect()
        } else {
            sidebar_agents
        };

    let has_mob = live_snapshot.has_mob_runtime;
    let identity_status_rows = build_identity_status_rows(&sidebar_agents);

    // P3: Build per-profile capability hints from roster data.
    let profile_capabilities: BTreeMap<String, Value> = {
        let mut profiles: BTreeMap<String, (usize, bool, bool)> = BTreeMap::new();
        for member in &live_snapshot.members {
            let entry = profiles
                .entry(member.role.clone())
                .or_insert((0, true, false));
            entry.0 += 1; // instance_count
            // addressable = all instances addressable
            let member_addressable = member
                .labels
                .get("addressable")
                .map(|v| v != "false")
                .unwrap_or(true);
            entry.1 = entry.1 && member_addressable;
            // has_wiring = any instance wired
            entry.2 = entry.2 || !member.wired_to.is_empty();
        }
        profiles
            .into_iter()
            .map(|(profile, (count, addressable, has_wiring))| {
                (
                    profile,
                    serde_json::json!({
                        "instance_count": count,
                        "addressable": addressable,
                        "has_wiring": has_wiring,
                    }),
                )
            })
            .collect()
    };

    serde_json::json!({
        "contract_version": MOBKIT_CONTRACT_VERSION,
        "runtime_id": live_snapshot.runtime_id,
        "runtime_capabilities": {
            "can_spawn_members": has_mob,
            "can_send_messages": has_mob,
            "can_wire_members": has_mob,
            "can_retire_members": has_mob,
            "available_spawn_modes": if has_mob {
                vec!["module", "role"]
            } else {
                vec!["module"]
            },
            "profile_capabilities": profile_capabilities,
        },
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
            "schema_version": "1",
            "refresh": {
                "mode": "poll",
                "interval_ms": 5000,
            },
            "source_method": if has_mob { "mobkit/list_members" } else { "mobkit/status" },
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
                "fields": ["agent_id", "member_id", "identity", "label", "kind", "role", "state", "response_phase", "wired_to", "labels", "group", "addressable", "affordances", "watched", "alertLevel", "degraded", "degradedReason"],
                "agent_id_field": "agent_id",
                "member_id_field": "member_id",
                "group_by_field": "group",
                "well_known_labels": {
                    "display_name": "human-readable label; overrides member_id in sidebar",
                    "addressable": "set \"false\" to hide send-message actions for internal agents",
                    "singleton": "set \"true\" to prevent retire (e.g. review, summarizer agents)",
                    "group": "sidebar group name; overrides profile-based grouping",
                },
                "refresh_projection": "source_method returns ConsoleMember rows (agent_identity, role, state, wired_to, labels). Clients must project: agent_id=agent_identity, member_id=agent_identity, label=labels.display_name||agent_identity, group=labels.group||role, addressable=labels.addressable!='false', affordances derived from labels.singleton and addressable.",
            },
            "live_snapshot": {
                "agents": sidebar_agents,
            }
        },
        "identity_status": {
            "panel_id": "console.identity_status",
            "title": "Identity Status",
            "schema_version": "1",
            "refresh": {
                "mode": "poll",
                "interval_ms": 5000,
            },
            "source_method": if has_mob { "mobkit/list_members" } else { "mobkit/status" },
            "rows": identity_status_rows,
        },
        "activity_feed": {
            "panel_id": "console.activity_feed",
            "title": "Activity",
            "schema_version": "1",
            "refresh": {
                "mode": "stream",
                "topic": "all_events",
                "update_semantics": "append",
            },
            "transport": "sse",
            "source_route": "/console/events/stream",
            "request_contract": {
                "last_event_id_header": "optional Last-Event-ID checkpoint from prior event_id",
            },
            "event_contract": {
                "envelope_fields": ["event_id", "interaction_id", "identity", "event_type", "timestamp_ms", "data"],
                "event_type_path": "event_type",
                "frame_format": "id: <event_id>\\nevent: <event_type>\\ndata: <event_json>\\n\\n",
            },
            "keep_alive": {
                "interval_ms": SSE_KEEP_ALIVE_INTERVAL_MS,
                "event": SSE_KEEP_ALIVE_EVENT_NAME,
                "comment_frame": SSE_KEEP_ALIVE_COMMENT_FRAME,
            },
            "filter_presets": [
                { "id": "all", "label": "All" },
                { "id": "watched-only", "label": "Watched only", "watchedOnly": true },
                { "id": "critical", "label": "Critical", "alertLevels": ["critical"] }
            ],
            "active_preset_id": "all"
        },
        "chat_inspector": {
            "panel_id": "console.chat_inspector",
            "title": "Chat Inspector",
            "schema_version": "1",
            "refresh": {
                "mode": "stream",
                "topic": "selected_identity",
                "update_semantics": "append",
            },
            "send_method": "mobkit/interact",
            "observe_route": "/console/identity/stream",
            "transport": "rpc+sse",
            "request_contract": {
                "identity": "required target identity",
                "content": "required user text to send",
                "origin": "required caller identifier for audit and routing",
            },
            "response_contract": {
                "interaction_id": "per-turn console correlation token",
                "identity": "echoed target identity",
            },
            "event_contract": {
                "envelope_fields": ["event_id", "interaction_id", "identity", "event_type", "timestamp_ms", "data"],
                "event_type_path": "event_type",
                "interaction_id_field": "interaction_id",
            }
        },
        "topology": {
            "panel_id": "console.topology",
            "title": "Topology",
            "schema_version": "1",
            "refresh": {
                "mode": "poll",
                "interval_ms": 5000,
            },
            "source_method": "mobkit/status",
            "route_method": "mobkit/routing/routes/list",
            "refresh_policy": {
                "mode": "pull",
                "poll_interval_ms": 5000,
            },
            "graph_contract": {
                "node_id_field": "identity",
                "edge_fields": ["wired_to"],
            },
            "live_snapshot": if live_snapshot.members.is_empty() {
                // Module-only runtime: topology nodes are loaded module IDs.
                serde_json::json!({
                    "nodes": &live_snapshot.loaded_modules,
                    "node_count": live_snapshot.loaded_modules.len(),
                })
            } else {
                // Mob runtime: identity-native topology from members.
                serde_json::json!({
                    "nodes": live_snapshot.members.iter().map(|member| {
                        let addressable = member
                            .labels
                            .get("addressable")
                            .map(|value| value != "false")
                            .unwrap_or(true);
                        serde_json::json!({
                            "identity": member.agent_identity,
                            "label": member.labels.get("display_name").cloned().unwrap_or_else(|| member.agent_identity.clone()),
                            "role": member.role,
                            "state": member.state,
                            "wired_to": member.wired_to,
                            "addressable": addressable,
                        })
                    }).collect::<Vec<_>>(),
                    "node_count": live_snapshot.members.len(),
                })
            }
        },
        "health_overview": {
            "panel_id": "console.health_overview",
            "title": "Health",
            "schema_version": "1",
            "refresh": {
                "mode": "poll",
                "interval_ms": 5000,
            },
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
                "loaded_module_count": &live_snapshot.loaded_modules.len(),
                "identities": identity_status_rows,
            }
        },
        "flows": {
            "panel_id": "console.flows",
            "title": "Flows",
            "schema_version": "1",
            "refresh": {
                "mode": "poll",
                "interval_ms": 10000,
            },
            "evaluate_method": "mobkit/scheduling/evaluate",
            "dispatch_method": "mobkit/scheduling/dispatch",
            "refresh_policy": {
                "mode": "pull",
                "poll_interval_ms": 10000,
            },
            "request_contract": {
                "schedules": "caller-supplied array of ScheduleDefinition (schedule_id, interval|cron, timezone, enabled)",
                "tick_ms": "evaluation timestamp in epoch milliseconds",
            },
            "evaluate_response_contract": {
                "tick_ms": "u64 — echoed evaluation tick",
                "due_triggers": "array of ScheduleTrigger {schedule_id, interval, timezone, due_tick_ms}",
            },
            "dispatch_response_contract": {
                "tick_ms": "u64 — echoed dispatch tick",
                "due_count": "usize — number of triggers that were due",
                "dispatched": "array of ScheduleDispatch {claim_key, schedule_id, interval, timezone, due_tick_ms, tick_ms, event_id, supervisor_signal?, runtime_injection?, runtime_injection_error?}",
                "skipped_claims": "array of schedule_id strings skipped due to idempotent claim",
            },
            "note": "Flows require caller-supplied schedule definitions; the runtime does not persist a flow registry. Clients must maintain their own schedule configs."
        },
        "session_history": if has_mob {
            serde_json::json!({
                "panel_id": "console.session_history",
                "title": "Session History",
                "schema_version": "1",
                "refresh": {
                    "mode": "poll",
                    "interval_ms": 5000,
                },
                "source_method": "mobkit/read_session_history",
                "transport": "rpc",
                "available": true,
                "request_contract": {
                    "session_id": "required current session id for the identity/member",
                    "offset": "optional message offset from the start of the transcript",
                    "limit": "optional max messages to return",
                },
                "response_contract": {
                    "success": "result is SessionHistoryPage {session_id, message_count, offset, limit, has_more, messages}",
                }
            })
        } else {
            serde_json::json!({
                "panel_id": "console.session_history",
                "title": "Session History",
                "schema_version": "1",
                "refresh": {
                    "mode": "poll",
                    "interval_ms": 5000,
                },
                "available": false,
                "reason": "session history requires a mob runtime with mobkit/query_events support",
            })
        }
    })
}

fn build_identity_status_rows(sidebar_agents: &[Value]) -> Vec<Value> {
    sidebar_agents
        .iter()
        .map(|agent| {
            let identity = agent
                .get("identity")
                .and_then(Value::as_str)
                .or_else(|| agent.get("member_id").and_then(Value::as_str))
                .unwrap_or_default();
            let display_name = agent
                .get("label")
                .and_then(Value::as_str)
                .filter(|label| *label != identity);
            let role = agent.get("role").and_then(Value::as_str);
            let state = agent
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let labels = agent
                .get("labels")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let addressability = if agent
                .get("addressable")
                .and_then(Value::as_bool)
                .unwrap_or(true)
            {
                "addressable"
            } else {
                "internal_only"
            };
            let mut row = serde_json::json!({
                "identity": identity,
                "state": state,
                "addressability": addressability,
                "labels": labels,
            });
            if let Some(display_name) = display_name {
                row["display_name"] = Value::String(display_name.to_string());
            }
            if let Some(role) = role {
                row["role"] = Value::String(role.to_string());
            }
            if let Some(generation) = agent.get("generation").and_then(Value::as_u64) {
                row["generation"] = Value::from(generation);
            }
            if let Some(checkpoint_version) =
                agent.get("checkpoint_version").and_then(Value::as_u64)
            {
                row["checkpoint_version"] = Value::from(checkpoint_version);
            }
            if let Some(lease_healthy) = agent.get("lease_healthy").and_then(Value::as_bool) {
                row["lease_healthy"] = Value::from(lease_healthy);
            }
            if let Some(response_phase) = agent.get("response_phase").and_then(Value::as_str) {
                row["response_phase"] = Value::String(response_phase.to_string());
            }
            if let Some(session_id) = agent.get("session_id").and_then(Value::as_str) {
                row["session_id"] = Value::String(session_id.to_string());
            }
            row
        })
        .collect()
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

/// Validate a bearer token against the console's trusted OIDC config AND
/// the email allowlist / provider policy (via `enforce_console_route_access`).
/// Returns `true` only if the token is valid AND the caller is authorized.
pub fn validate_console_token(decisions: &RuntimeDecisionState, token: &str) -> bool {
    let auth = match resolve_console_auth_from_token(decisions, token) {
        Ok(auth) => auth,
        Err(_) => return false,
    };
    crate::decisions::enforce_console_route_access(&decisions.auth, &decisions.console, &auth)
        .is_ok()
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
