//! Edge topology reconciliation for distributed runtime nodes.

use std::collections::{BTreeMap, BTreeSet};

use futures::stream::{self, StreamExt};
use meerkat_mob::SpawnMemberSpec;
use meerkat_mob::ids::AgentIdentity;
use meerkat_mob::runtime::MobMemberListEntry;
use meerkat_mob::runtime::reconcile::ReconcileOptions;

use crate::runtime::RuntimeRoute;
use crate::unified_runtime::types::meerkat_reconcile_report_to_wire;

use super::edge_types::{DesiredPeerEdge, EdgeMemberView, EdgeReconcileFailure};
use super::types::{
    UnifiedRuntimeReconcileEdgesReport, UnifiedRuntimeReconcileError,
    UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileRoutingReport,
};
use super::{
    ROSTER_ROUTE_CHANNEL, ROSTER_ROUTE_PREFIX, ROSTER_ROUTE_SINK, ROSTER_ROUTE_TARGET_MODULE,
    UnifiedRuntime,
};

const EDGE_RECONCILE_CONCURRENCY: usize = 64;

impl UnifiedRuntime {
    pub async fn reconcile(
        &self,
        desired_specs: Vec<SpawnMemberSpec>,
    ) -> Result<UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileError> {
        self.mob_runtime
            .set_baseline_member_specs(desired_specs.clone())
            .await;
        // Spec member ids arrive in the public alias space (identity-first
        // identities like `domain:billing` contain `:`); encode them into
        // comms-safe roster ids at the mobkit→meerkat-mob boundary (meerkat
        // 0.7 `MemberCommsName` is fail-closed). The projections below —
        // reconcile report, console identity registration, routing — decode
        // back to the alias space.
        let desired_specs: Vec<SpawnMemberSpec> = desired_specs
            .into_iter()
            .map(|mut spec| {
                spec.identity = crate::member_comms_id::mob_member_id(spec.identity.as_str());
                spec
            })
            .collect();
        // 1. Member reconcile (meerkat 0.6 native path)
        let mob_id = self.mob_handle().mob_id().to_string();
        let meerkat_report = self
            .mob_handle()
            .reconcile(desired_specs, ReconcileOptions { retire_stale: true })
            .await
            .map_err(|err| UnifiedRuntimeReconcileError::Mob(err.into()))?;
        let mob = meerkat_reconcile_report_to_wire(&mob_id, meerkat_report);
        // 2. Refresh active members. Roster ids are comms-safe encodings; the
        // agent-event ingest decodes them back to the alias space, so console
        // registration is keyed by the public alias. Register as a fallback
        // only: spawn/reserve paths register the durable console identity for
        // the same alias key, and a reconcile must never clobber that with
        // the alias self-mapping.
        let active_snapshots = self.mob_handle().list_members_including_retiring().await;
        for member in &active_snapshots {
            let alias = crate::member_comms_id::runtime_alias_str(member.agent_identity.as_str())
                .into_owned();
            self.console_events
                .register_runtime_identity_fallback(alias.clone(), alias)
                .await;
        }
        let active_member_ids = active_snapshots
            .iter()
            .map(|m| {
                crate::member_comms_id::runtime_alias_str(m.agent_identity.as_str()).into_owned()
            })
            .collect::<Vec<_>>();
        // 3 + 4. Edge discovery + dynamic edge reconcile
        let edges = self.reconcile_edges_from_members(active_snapshots).await;
        // 5. Routing reconcile
        let routing = self.reconcile_routing_wiring(active_member_ids).await?;
        let report = UnifiedRuntimeReconcileReport {
            mob,
            edges,
            routing,
        };
        if let Some(hook) = &self.post_reconcile_hook {
            hook(report.clone()).await;
        }
        // Meerkat 0.6's `MobHandle::reconcile` returns Ok even on per-identity
        // failures (they're collected into `report.mob.failures`). Re-lift any
        // non-empty failures list into an `Err` so callers using `?` see the
        // same propagation behavior they had pre-cleanup, while still carrying
        // the full report for inspection via `PartialFailure`.
        if !report.mob.failures.is_empty() {
            return Err(UnifiedRuntimeReconcileError::PartialFailure(Box::new(
                report,
            )));
        }
        Ok(report)
    }

    /// Reconcile dynamic peer edges using fresh roster state.
    ///
    /// Refreshes the roster, runs edge discovery if configured, diffs
    /// desired vs managed edges, and calls wire/unwire as needed.
    pub async fn reconcile_edges(&self) -> UnifiedRuntimeReconcileEdgesReport {
        let active_members = self.mob_handle().list_members_including_retiring().await;
        let report = self.reconcile_edges_from_members(active_members).await;
        if !report.is_complete() {
            self.fire_error(super::types::ErrorEvent::ReconcileIncomplete {
                failures: report.failures.len(),
                skipped: report.skipped_missing_members.len(),
            });
        }
        report
    }

    pub(super) async fn reconcile_edges_from_members(
        &self,
        active_members: Vec<MobMemberListEntry>,
    ) -> UnifiedRuntimeReconcileEdgesReport {
        let edge_discovery = match &self.edge_discovery {
            Some(d) => d,
            None => return UnifiedRuntimeReconcileEdgesReport::default(),
        };

        // Everything in this function speaks the public alias space: roster
        // ids are comms-safe encodings (meerkat 0.7 `MemberCommsName`), so
        // decode snapshots on entry and encode only at the wire/unwire calls.
        let alias_of =
            |id: &str| -> String { crate::member_comms_id::runtime_alias_str(id).into_owned() };

        let active_ids: BTreeSet<String> = active_members
            .iter()
            .map(|m| alias_of(m.agent_identity.as_str()))
            .collect();

        // Build current wiring map from snapshots
        let mut current_edges: BTreeSet<(String, String)> = BTreeSet::new();
        for member in &active_members {
            for peer in &member.wired_to {
                let mut a = alias_of(member.agent_identity.as_str());
                let mut b = alias_of(peer.as_str());
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                current_edges.insert((a, b));
            }
        }

        // Project to EdgeMemberView for the policy closure — it only needs
        // identity/role/labels/wired_to, not meerkat's private runtime fields.
        let member_views: Vec<EdgeMemberView> = active_members
            .into_iter()
            .map(|m| EdgeMemberView {
                agent_identity: alias_of(m.agent_identity.as_str()),
                role: m.role.to_string(),
                wired_to: m
                    .wired_to
                    .iter()
                    .map(|peer| alias_of(peer.as_str()))
                    .collect(),
                labels: m.labels,
            })
            .collect();

        // Run edge discovery
        let raw_desired = edge_discovery.discover_edges(member_views).await;

        // Deduplicate and defensively validate (DesiredPeerEdge enforces
        // invariants at construction, but we still canonicalize the key set)
        let desired: BTreeSet<(String, String)> = raw_desired
            .iter()
            .map(|e| {
                let (a, b) = e.endpoints();
                (a.to_string(), b.to_string())
            })
            .collect();

        let mut report = UnifiedRuntimeReconcileEdgesReport {
            desired_edges: raw_desired,
            ..Default::default()
        };

        let managed_snapshot = self.managed_dynamic_edges.read().await.clone();
        let mut to_wire = Vec::new();

        // Classify desired edges
        for (a, b) in &desired {
            // Skip if either endpoint is missing from the active roster
            if !active_ids.contains(a) || !active_ids.contains(b) {
                if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                    report.skipped_missing_members.push(edge);
                }
                continue;
            }
            let key = (a.clone(), b.clone());
            if managed_snapshot.contains(&key) {
                // Managed by us — check if the actual edge still exists in the
                // mob graph. If an out-of-band unwire() removed it, re-wire.
                if current_edges.contains(&key) {
                    if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                        report.retained_edges.push(edge);
                    }
                } else {
                    to_wire.push((a.clone(), b.clone(), "wire (heal)"));
                }
            } else if current_edges.contains(&key) {
                // Exists but not managed by us (static or external) — don't claim
                if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                    report.preexisting_edges.push(edge);
                }
            } else {
                to_wire.push((a.clone(), b.clone(), "wire"));
            }
        }

        // Unwire managed edges that are no longer desired
        let mut stale_pruned = Vec::new();
        let mut to_unwire = Vec::new();
        for (a, b) in managed_snapshot
            .iter()
            .filter(|key| !desired.contains(*key))
            .cloned()
        {
            let key = (a.clone(), b.clone());
            // If either endpoint is gone, just prune from managed set
            if !active_ids.contains(&a) || !active_ids.contains(&b) {
                stale_pruned.push((a, b));
                continue;
            }
            // If the edge is already gone from the mob graph (out-of-band
            // unwire/reset), just drop ownership — don't attempt unwire.
            if !current_edges.contains(&key) {
                stale_pruned.push((a, b));
                continue;
            }
            to_unwire.push((a, b));
        }

        let handle = self.mob_handle();
        let wire_operations = to_wire
            .iter()
            .map(|(a, b, operation)| ((a.clone(), b.clone()), (*operation).to_string()))
            .collect::<BTreeMap<_, _>>();
        let mut wire_successes = Vec::new();
        let mut wire_failures = Vec::new();
        if !to_wire.is_empty() {
            let batch_edges = to_wire
                .iter()
                .map(|(a, b, _)| {
                    (
                        crate::member_comms_id::mob_member_id(a.as_str()),
                        crate::member_comms_id::mob_member_id(b.as_str()),
                    )
                })
                .collect::<Vec<_>>();
            // The batch report carries roster ids; decode back to aliases and
            // re-canonicalize (alias-space ordering can differ from roster-id
            // ordering) so keys line up with `to_wire`.
            let alias_edge_key = |a: &AgentIdentity, b: &AgentIdentity| -> (String, String) {
                let mut a = crate::member_comms_id::runtime_alias_str(a.as_str()).into_owned();
                let mut b = crate::member_comms_id::runtime_alias_str(b.as_str()).into_owned();
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                (a, b)
            };
            match handle.wire_members_batch(batch_edges).await {
                Ok(batch_report) => {
                    let mut seen = BTreeSet::new();
                    for edge in batch_report.wired {
                        let key = alias_edge_key(&edge.a, &edge.b);
                        seen.insert(key.clone());
                        wire_successes.push((key.0, key.1, true));
                    }
                    for edge in batch_report.already_wired {
                        let key = alias_edge_key(&edge.a, &edge.b);
                        seen.insert(key.clone());
                        wire_successes.push((key.0, key.1, false));
                    }
                    for (a, b, operation) in to_wire {
                        let key = if a <= b { (a, b) } else { (b, a) };
                        if !seen.contains(&key) {
                            wire_failures.push((
                                key.0,
                                key.1,
                                operation.to_string(),
                                "wire_members_batch omitted edge from report".to_string(),
                            ));
                        }
                    }
                }
                Err(err) => {
                    let error = err.to_string();
                    for (a, b, operation) in to_wire {
                        wire_failures.push((a, b, operation.to_string(), error.clone()));
                    }
                }
            }
        }

        let handle = self.mob_handle();
        let unwire_results = stream::iter(to_unwire.into_iter().map(|(a, b)| {
            let handle = handle.clone();
            async move {
                let result = handle
                    .unwire(
                        crate::member_comms_id::mob_member_id(a.as_str()),
                        crate::member_comms_id::mob_member_id(b.as_str()),
                    )
                    .await
                    .map_err(|err| format!("{err}"));
                (a, b, result)
            }
        }))
        .buffer_unordered(EDGE_RECONCILE_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

        let mut managed_edges = self.managed_dynamic_edges.write().await;
        for (a, b, newly_wired) in wire_successes {
            managed_edges.insert((a.clone(), b.clone()));
            if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                if newly_wired {
                    report.wired_edges.push(edge);
                } else {
                    report.retained_edges.push(edge);
                }
            }
        }
        for (a, b, operation, error) in wire_failures {
            if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                report.failures.push(EdgeReconcileFailure {
                    edge,
                    operation: wire_operations.get(&(a, b)).cloned().unwrap_or(operation),
                    error,
                });
            }
        }
        for (a, b) in stale_pruned {
            managed_edges.remove(&(a.clone(), b.clone()));
            if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                report.pruned_stale_managed_edges.push(edge);
            }
        }
        for (a, b, result) in unwire_results {
            match result {
                Ok(()) => {
                    managed_edges.remove(&(a.clone(), b.clone()));
                    if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                        report.unwired_edges.push(edge);
                    }
                }
                Err(error) => {
                    if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                        report.failures.push(EdgeReconcileFailure {
                            edge,
                            operation: "unwire".into(),
                            error,
                        });
                    }
                }
            }
        }

        report
    }

    pub(super) async fn reconcile_routing_wiring(
        &self,
        mut active_members: Vec<String>,
    ) -> Result<UnifiedRuntimeReconcileRoutingReport, UnifiedRuntimeReconcileError> {
        active_members.sort();
        active_members.dedup();

        let mut rt = self.module_runtime.lock().await;
        let router_module_loaded = rt
            .loaded_modules()
            .iter()
            .any(|module_id| module_id == "router");
        let mut added_route_keys = Vec::new();
        let mut removed_route_keys = Vec::new();

        if router_module_loaded {
            let managed_routes: Vec<RuntimeRoute> = rt
                .list_runtime_routes()
                .into_iter()
                .filter(|route| route.route_key.starts_with(ROSTER_ROUTE_PREFIX))
                .collect();
            let active_member_set = active_members.iter().cloned().collect::<BTreeSet<_>>();
            for route in &managed_routes {
                if !active_member_set.contains(&route.recipient) {
                    rt.delete_runtime_route(&route.route_key)
                        .map_err(UnifiedRuntimeReconcileError::RouteMutation)?;
                    removed_route_keys.push(route.route_key.clone());
                }
            }

            let existing_managed_recipients = managed_routes
                .into_iter()
                .map(|route| route.recipient)
                .collect::<BTreeSet<_>>();
            for member_id in &active_members {
                if existing_managed_recipients.contains(member_id) {
                    continue;
                }
                let route_key = format!("{ROSTER_ROUTE_PREFIX}{member_id}");
                rt.add_runtime_route(RuntimeRoute {
                    route_key: route_key.clone(),
                    recipient: member_id.clone(),
                    channel: Some(ROSTER_ROUTE_CHANNEL.to_string()),
                    sink: ROSTER_ROUTE_SINK.to_string(),
                    target_module: ROSTER_ROUTE_TARGET_MODULE.to_string(),
                    retry_max: None,
                    backoff_ms: None,
                    rate_limit_per_minute: None,
                })
                .map_err(UnifiedRuntimeReconcileError::RouteMutation)?;
                added_route_keys.push(route_key);
            }
        }

        added_route_keys.sort();
        removed_route_keys.sort();

        Ok(UnifiedRuntimeReconcileRoutingReport {
            router_module_loaded,
            active_members,
            added_route_keys,
            removed_route_keys,
        })
    }
}
