//! `mobkit/storage/doctor` dispatch, shared by the module-only stdin RPC,
//! the unified stdin RPC, and the console RPC surfaces.
//!
//! Read-only by contract (see [`crate::storage_doctor`]'s safety contract);
//! the console registers it as a read method. Params carry an explicit
//! `state_dir` because no runtime handle exposes its persistent state
//! directory yet — the storage layout authority that would provide the
//! default lands in Phase M2. Until then, runtime-backed surfaces answer a
//! missing `state_dir` with the typed capability-unavailable error
//! ([`storage_doctor_state_dir_unavailable_error`], code `-32004`).

use std::path::PathBuf;

use meerkat_core::storage_diagnostics::{DiagnoseScope, StorageDiagnosis};
use serde_json::Value;

use crate::storage_doctor;
use crate::storage_health::ResolvedStorageSummary;

use super::JsonRpcError;

/// The doctor RPC method name (read method on every surface).
pub(crate) const STORAGE_DOCTOR_METHOD: &str = "mobkit/storage/doctor";

/// Parsed `mobkit/storage/doctor` params.
pub(crate) struct StorageDoctorParams {
    pub state_dir: PathBuf,
    /// Optional identity filter for the continuity checkpoint census
    /// (mapped onto [`DiagnoseScope::realm`]).
    pub identity: Option<String>,
}

impl StorageDoctorParams {
    pub(crate) fn scope(&self) -> DiagnoseScope {
        let scope = DiagnoseScope::new(vec![self.state_dir.clone()]);
        match &self.identity {
            Some(identity) => scope.with_realm(identity.clone()),
            None => scope,
        }
    }
}

/// Parse doctor params. `Ok(None)` = no `state_dir` given — the caller maps
/// that to its surface's typed error; `Err` = a param is present but
/// mistyped (`-32602` on every surface).
pub(crate) fn parse_storage_doctor_params(
    params: &Value,
) -> Result<Option<StorageDoctorParams>, String> {
    if !params.is_null() && !params.is_object() {
        return Err("params must be an object".to_string());
    }
    let state_dir = match params.get("state_dir") {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(dir)) if !dir.trim().is_empty() => PathBuf::from(dir),
        Some(_) => return Err("state_dir must be a non-empty string".to_string()),
    };
    let identity = match params.get("identity") {
        None | Some(Value::Null) => None,
        Some(Value::String(identity)) => Some(identity.clone()),
        Some(_) => return Err("identity must be a string".to_string()),
    };
    Ok(Some(StorageDoctorParams {
        state_dir,
        identity,
    }))
}

/// The typed error a runtime-backed surface returns when no `state_dir` is
/// given: `-32004` (the SDKs' reserved capability-unavailable code) because
/// the runtime cannot yet report its own persistent state directory.
pub(crate) fn storage_doctor_state_dir_unavailable_error() -> JsonRpcError {
    JsonRpcError {
        code: -32004,
        message: "storage doctor requires params.state_dir: the runtime does not expose its \
                  persistent state directory (the storage layout authority lands in Phase M2)"
            .to_string(),
        data: None,
    }
}

/// The doctor result payload: the serialized [`StorageDiagnosis`] plus the
/// live H1/H2 storage summary when the invoking surface has one.
pub(crate) fn storage_doctor_result_json(
    params: &StorageDoctorParams,
    diagnosis: &StorageDiagnosis,
    resolved: Option<ResolvedStorageSummary>,
) -> Value {
    serde_json::json!({
        "state_dir": params.state_dir.display().to_string(),
        "diagnosis": serde_json::to_value(diagnosis).unwrap_or(Value::Null),
        "storage": resolved
            .map(|summary| summary.status_json())
            .unwrap_or(Value::Null),
    })
}

/// Run the doctor for a runtime-backed surface: live durability census when
/// the runtime resolved its storage at composition time.
pub(crate) async fn run_storage_doctor(
    params: &StorageDoctorParams,
    resolved: Option<ResolvedStorageSummary>,
) -> Value {
    let diagnosis =
        storage_doctor::diagnose_state_dir_with_runtime(&params.scope(), resolved).await;
    storage_doctor_result_json(params, &diagnosis, resolved)
}
