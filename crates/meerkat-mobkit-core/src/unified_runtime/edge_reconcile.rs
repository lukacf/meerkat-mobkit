use std::collections::BTreeSet;

use meerkat_mob::{MeerkatId, SpawnMemberSpec};

use crate::mob_handle_runtime::MobMemberSnapshot;
use crate::runtime::RuntimeRoute;

use super::edge_types::DesiredPeerEdge;
use super::types::{
    UnifiedRuntimeReconcileEdgesReport, UnifiedRuntimeReconcileError,
    UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileRoutingReport,
};
use super::edge_types::EdgeReconcileFailure;
use super::{UnifiedRuntime, ROSTER_ROUTE_CHANNEL, ROSTER_ROUTE_PREFIX, ROSTER_ROUTE_SINK, ROSTER_ROUTE_TARGET_MODULE};

impl UnifiedRuntime {
    pub async fn reconcile(
        &self,
        desired_specs: Vec<SpawnMemberSpec>,
    ) -> Result<UnifiedRuntimeReconcileReport, UnifiedRuntimeReconcileError> {
        // 1. Member reconcile
        let mob = self
            .mob_runtime
            .reconcile(desired_specs)
            .await
            .map_err(UnifiedRuntimeReconcileError::Mob)?;
        // 2. Refresh active members
        let active_snapshots = self.mob_runtime.discover().await;
        let active_member_ids = active_snapshots
            .iter()
            .map(|m| m.meerkat_id.clone())
            .collect::<Vec<_>>();
        // 3 + 4. Edge discovery + dynamic edge reconcile
        let edges = self
            .reconcile_edges_from_members(active_snapshots)
            .await;
        // 5. Routing reconcile
        let routing = self.reconcile_routing_wiring(active_member_ids)?;
        let report = UnifiedRuntimeReconcileReport { mob, edges, routing };
        if let Some(hook) = &self.post_reconcile_hook {
            hook(report.clone()).await;
        }
        Ok(report)
    }

    /// Reconcile dynamic peer edges using fresh roster state.
    ///
    /// Refreshes the roster, runs edge discovery if configured, diffs
    /// desired vs managed edges, and calls wire/unwire as needed.
    pub async fn reconcile_edges(
        &self,
    ) -> UnifiedRuntimeReconcileEdgesReport {
        let active_members = self.mob_runtime.discover().await;
        self.reconcile_edges_from_members(active_members).await
    }

    pub(super) async fn reconcile_edges_from_members(
        &self,
        active_members: Vec<MobMemberSnapshot>,
    ) -> UnifiedRuntimeReconcileEdgesReport {
        let edge_discovery = match &self.edge_discovery {
            Some(d) => d,
            None => return UnifiedRuntimeReconcileEdgesReport::default(),
        };

        let active_ids: BTreeSet<String> = active_members
            .iter()
            .map(|m| m.meerkat_id.clone())
            .collect();

        // Build current wiring map from snapshots
        let mut current_edges: BTreeSet<(String, String)> = BTreeSet::new();
        for member in &active_members {
            for peer in &member.wired_to {
                let mut a = member.meerkat_id.clone();
                let mut b = peer.clone();
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                current_edges.insert((a, b));
            }
        }

        // Run edge discovery
        let raw_desired = edge_discovery.discover_edges(active_members).await;

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

        // Write lock for managed_dynamic_edges — held across awaits
        // because wire/unwire must be serialized with edge set mutations.
        let mut managed_edges = self.managed_dynamic_edges.write().await;

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
            if managed_edges.contains(&key) {
                // Managed by us — check if the actual edge still exists in the
                // mob graph. If an out-of-band unwire() removed it, re-wire.
                if current_edges.contains(&key) {
                    if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                        report.retained_edges.push(edge);
                    }
                } else {
                    // Managed edge disappeared from mob graph — heal it
                    let mid_a = MeerkatId::from(a.as_str());
                    let mid_b = MeerkatId::from(b.as_str());
                    match self.mob_runtime.handle().wire(mid_a, mid_b).await {
                        Ok(()) => {
                            if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                                report.wired_edges.push(edge);
                            }
                        }
                        Err(err) => {
                            if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                                report.failures.push(EdgeReconcileFailure {
                                    edge,
                                    operation: "wire (heal)".into(),
                                    error: format!("{err}"),
                                });
                            }
                        }
                    }
                }
            } else if current_edges.contains(&key) {
                // Exists but not managed by us (static or external) — don't claim
                if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                    report.preexisting_edges.push(edge);
                }
            } else {
                // New edge — wire it
                let mid_a = MeerkatId::from(a.as_str());
                let mid_b = MeerkatId::from(b.as_str());
                match self.mob_runtime.handle().wire(mid_a, mid_b).await {
                    Ok(()) => {
                        managed_edges.insert(key);
                        if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                            report.wired_edges.push(edge);
                        }
                    }
                    Err(err) => {
                        if let Ok(edge) = DesiredPeerEdge::new(a.clone(), b.clone()) {
                            report.failures.push(EdgeReconcileFailure {
                                edge,
                                operation: "wire".into(),
                                error: format!("{err}"),
                            });
                        }
                    }
                }
            }
        }

        // Unwire managed edges that are no longer desired
        let to_unwire: Vec<(String, String)> = managed_edges
            .iter()
            .filter(|key| !desired.contains(*key))
            .cloned()
            .collect();

        for (a, b) in to_unwire {
            let key = (a.clone(), b.clone());
            // If either endpoint is gone, just prune from managed set
            if !active_ids.contains(&a) || !active_ids.contains(&b) {
                managed_edges.remove(&key);
                if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                    report.pruned_stale_managed_edges.push(edge);
                }
                continue;
            }
            // If the edge is already gone from the mob graph (out-of-band
            // unwire/reset), just drop ownership — don't attempt unwire.
            if !current_edges.contains(&key) {
                managed_edges.remove(&key);
                if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                    report.pruned_stale_managed_edges.push(edge);
                }
                continue;
            }
            let mid_a = MeerkatId::from(a.as_str());
            let mid_b = MeerkatId::from(b.as_str());
            match self.mob_runtime.handle().unwire(mid_a, mid_b).await {
                Ok(()) => {
                    managed_edges.remove(&key);
                    if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                        report.unwired_edges.push(edge);
                    }
                }
                Err(err) => {
                    if let Ok(edge) = DesiredPeerEdge::new(a, b) {
                        report.failures.push(EdgeReconcileFailure {
                            edge,
                            operation: "unwire".into(),
                            error: format!("{err}"),
                        });
                    }
                }
            }
        }

        report
    }

    pub(super) fn reconcile_routing_wiring(
        &self,
        mut active_members: Vec<String>,
    ) -> Result<UnifiedRuntimeReconcileRoutingReport, UnifiedRuntimeReconcileError> {
        active_members.sort();
        active_members.dedup();

        let mut rt = self.module_runtime.lock().unwrap_or_else(|e| e.into_inner());
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
                    rt
                        .delete_runtime_route(&route.route_key)
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
                rt
                    .add_runtime_route(RuntimeRoute {
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
