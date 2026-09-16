//! Console request ownership around the shared Meerkat live host.
//!
//! This registry fences HTTP retries and cancellation, not live execution.
//! A host owns every channel, receipt, credential, and provider effect.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::{Mutex, Notify};

use crate::live_contracts::PendingLiveChannelHandle;

#[cfg(feature = "openai-live")]
mod auth;
#[cfg(feature = "openai-live")]
mod live_host;
mod summary;

pub(crate) const VOICE_OPEN_METHOD: &str = "mobkit/console/voice/open";
pub(crate) const VOICE_READINESS_METHOD: &str = "mobkit/console/voice/readiness";
pub(crate) const VOICE_CLOSE_METHOD: &str = "mobkit/console/voice/close";
pub(crate) const VOICE_ANSWER_RECEIVED_METHOD: &str = "mobkit/console/voice/answer_received";
pub(crate) const VOICE_REPLACEMENT_METHOD: &str = "mobkit/console/voice/replacement";
pub(crate) const VOICE_ACTIVITY_METHOD: &str = "mobkit/console/voice/activity";
const SILENCE_LIMIT: Duration = Duration::from_mins(15);
const PENDING_SETUP_LIMIT: Duration = Duration::from_mins(2);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoiceActivity {
    pub identity: String,
    pub request_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoiceReadiness {
    pub identity: String,
}

pub(crate) fn is_channel_method(method: &str) -> bool {
    matches!(
        method,
        "mobkit/live/playback_owner/register"
            | "mobkit/live/playback_owner/revoke"
            | "mobkit/live/status"
            | "mobkit/live/close"
            | "mobkit/live/refresh"
            | "mobkit/live/interrupt"
            | "live/webrtc/answer"
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoiceAnswerReceived {
    pub identity: String,
    pub request_id: String,
    pub channel_id: String,
}
const MAX_REQUESTS: usize = 4096;
const CLOSE_WAIT: Duration = Duration::from_secs(10);
pub const CONSOLE_VOICE_SHUTDOWN_TIMEOUT: Duration = CLOSE_WAIT;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct VoiceRequest {
    pub identity: String,
    pub request_id: String,
}

impl VoiceRequest {
    fn validate(&self) -> Result<(), VoiceError> {
        validate_identity(&self.identity)?;
        if !valid_request_atom(&self.request_id) {
            return Err(VoiceError::InvalidRequest);
        }
        Ok(())
    }
}

fn valid_request_atom(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && value.trim() == value
}

fn validate_identity(identity: &str) -> Result<(), VoiceError> {
    if !valid_request_atom(identity)
        || crate::member_comms_id::is_reserved_generated_alias(identity)
    {
        return Err(VoiceError::InvalidRequest);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VoiceError {
    Unavailable,
    Unauthorized,
    InvalidRequest,
    RequestConflict,
    RequestCapacity,
    Cancelled,
    Closed,
    Busy,
    HostFailed,
}

impl VoiceError {
    pub(crate) fn rpc_error(self) -> crate::rpc::JsonRpcError {
        let (code, kind, message) = match self {
            Self::Unavailable => (-32050, "voice_unavailable", "Console voice is unavailable"),
            Self::Unauthorized => (
                -32030,
                "access_denied",
                "Console voice requires authorization",
            ),
            Self::InvalidRequest => (-32602, "invalid_params", "Invalid voice request"),
            Self::RequestConflict => (
                -32000,
                "voice_request_conflict",
                "Voice request conflicts with its existing owner",
            ),
            Self::RequestCapacity => (
                -32000,
                "voice_request_capacity",
                "Voice request capacity reached",
            ),
            Self::Cancelled => (-32000, "voice_cancelled", "Voice request was cancelled"),
            Self::Closed => (-32000, "voice_closed", "Voice conversation is closed"),
            Self::Busy => (
                -32000,
                "voice_busy",
                "Voice teardown is still pending; retry the same request",
            ),
            Self::HostFailed => (-32000, "voice_host_failed", "Voice host operation failed"),
        };
        crate::rpc::JsonRpcError {
            code,
            message: message.to_string(),
            data: Some(serde_json::json!({ "kind": kind })),
        }
    }
}

/// These methods must delegate to shared live authority. A successful close
/// means no later activation can emerge from this exact host-owned open.
#[async_trait]
pub(crate) trait ConsoleVoiceSession: Send + Sync {
    fn pending(&self) -> PendingLiveChannelHandle;
    async fn close(&self) -> Result<(), VoiceError>;
    async fn dispatch(
        &self,
        _method: &str,
        _params: serde_json::Value,
    ) -> Result<serde_json::Value, VoiceError> {
        Err(VoiceError::Unavailable)
    }
    async fn answer_received(&self, _channel: &str) -> Result<(), VoiceError> {
        Err(VoiceError::Unavailable)
    }
    async fn replacement_required(&self) -> Result<serde_json::Value, VoiceError> {
        Err(VoiceError::Unavailable)
    }
}

#[async_trait]
pub(crate) trait ConsoleVoiceHost: Send + Sync {
    /// Validate current target authorization and the configured OpenAI
    /// credential using the same authority as open, without opening a provider.
    async fn ready(&self, principal: &str, identity: &str) -> Result<bool, VoiceError>;
    async fn open(
        &self,
        principal: &str,
        identity: &str,
    ) -> Result<Arc<dyn ConsoleVoiceSession>, VoiceError>;
}

#[derive(Default)]
struct RequestState {
    opening: bool,
    cancelled: bool,
    closing: bool,
    closed: bool,
    session: Option<Arc<dyn ConsoleVoiceSession>>,
    open_error: Option<VoiceError>,
    close_error: Option<VoiceError>,
    last_activity: Option<tokio::time::Instant>,
    setup_deadline: Option<tokio::time::Instant>,
    activated: bool,
}

struct RequestSlot {
    identity: String,
    state: Mutex<RequestState>,
    changed: Notify,
}

impl RequestSlot {
    fn supervise(self: &Arc<Self>) {
        let slot = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let changed = slot.changed.notified();
                let mut state = slot.state.lock().await;
                if state.closed {
                    return;
                }
                if state.cancelled {
                    drop(state);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    slot.cancel().await;
                    continue;
                }
                let deadline = if state.activated {
                    state.last_activity.map(|activity| activity + SILENCE_LIMIT)
                } else {
                    state.setup_deadline
                };
                let Some(deadline) = deadline else {
                    drop(state);
                    changed.await;
                    continue;
                };
                if tokio::time::Instant::now() >= deadline {
                    state.cancelled = true;
                    drop(state);
                    slot.cancel().await;
                    continue;
                }
                drop(state);
                tokio::select! {
                    () = changed => {},
                    () = tokio::time::sleep_until(deadline) => {},
                }
            }
        });
    }

    fn new(identity: String, state: RequestState) -> Self {
        Self {
            identity,
            state: Mutex::new(state),
            changed: Notify::new(),
        }
    }

    async fn result(&self) -> Result<PendingLiveChannelHandle, VoiceError> {
        loop {
            let changed = self.changed.notified();
            let state = self.state.lock().await;
            if state.cancelled {
                return Err(VoiceError::Cancelled);
            }
            if let Some(error) = state.open_error {
                return Err(error);
            }
            if let Some(session) = state.session.as_ref() {
                return Ok(session.pending());
            }
            drop(state);
            changed.await;
        }
    }

    async fn wait_closed(&self) -> Result<(), VoiceError> {
        loop {
            let changed = self.changed.notified();
            let state = self.state.lock().await;
            if state.closed {
                return Ok(());
            }
            if let Some(error) = state.close_error {
                return Err(error);
            }
            drop(state);
            changed.await;
        }
    }

    async fn cancel(self: &Arc<Self>) {
        let mut state = self.state.lock().await;
        state.cancelled = true;
        self.changed.notify_waiters();
        if state.closed || state.closing {
            return;
        }
        state.closing = true;
        state.close_error = None;
        let slot = Arc::clone(self);
        // The request task does not own cleanup: dropping its HTTP response
        // must not cancel teardown or strand a late successful provider open.
        tokio::spawn(async move {
            let session = loop {
                let changed = slot.changed.notified();
                let state = slot.state.lock().await;
                if !state.opening {
                    break state.session.clone();
                }
                drop(state);
                changed.await;
            };
            let result = match session {
                Some(session) => session.close().await,
                None => Ok(()),
            };
            let mut state = slot.state.lock().await;
            state.closing = false;
            match result {
                Ok(()) => {
                    state.closed = true;
                    state.session = None;
                }
                Err(error) => state.close_error = Some(error),
            }
            slot.changed.notify_waiters();
        });
    }
}

type RequestKey = (String, String);

#[derive(Clone, Default)]
pub struct ConsoleVoiceController {
    host: Option<Arc<dyn ConsoleVoiceHost>>,
    requests: Arc<Mutex<HashMap<RequestKey, Arc<RequestSlot>>>>,
    stopped: Arc<AtomicBool>,
}

impl ConsoleVoiceController {
    pub async fn shutdown(&self) -> Result<(), String> {
        let drain = async {
            let requests = self.requests.lock().await;
            self.stopped.store(true, Ordering::SeqCst);
            let slots = requests.values().cloned().collect::<Vec<_>>();
            drop(requests);
            for slot in &slots {
                slot.cancel().await;
            }
            for result in
                futures::future::join_all(slots.iter().map(|slot| slot.wait_closed())).await
            {
                result.map_err(|_| "console voice cleanup failed".to_string())?;
            }
            Ok(())
        };
        tokio::time::timeout(CONSOLE_VOICE_SHUTDOWN_TIMEOUT, drain)
            .await
            .map_err(|_| "console voice cleanup remains pending".to_string())?
    }

    pub(crate) async fn note_activity(
        &self,
        principal: &str,
        request: VoiceActivity,
    ) -> Result<(), VoiceError> {
        let slot = self
            .request_slot(
                principal,
                &VoiceRequest {
                    identity: request.identity,
                    request_id: request.request_id,
                },
            )
            .await?;
        let mut state = slot.state.lock().await;
        if state.cancelled || state.closed {
            return Err(VoiceError::Cancelled);
        }
        if state.session.is_none() || !state.activated {
            return Err(VoiceError::Busy);
        }
        state.last_activity = Some(tokio::time::Instant::now());
        tracing::trace!("console voice audio activity accepted");
        slot.changed.notify_waiters();
        Ok(())
    }

    /// Authentication readiness is independent of whether a voice request
    /// already owns the target; it is not permission to open a second channel.
    pub(crate) async fn ready(&self, principal: &str, identity: &str) -> Result<bool, VoiceError> {
        validate_identity(identity)?;
        if principal.trim().is_empty() {
            return Err(VoiceError::Unauthorized);
        }
        if self.stopped.load(Ordering::SeqCst) {
            return Ok(false);
        }
        match &self.host {
            Some(host) => host.ready(principal, identity).await,
            None => Ok(false),
        }
    }

    pub(crate) fn configured(&self) -> bool {
        self.host.is_some() && !self.stopped.load(Ordering::SeqCst)
    }

    async fn owned_session(
        &self,
        principal: &str,
        request: &VoiceRequest,
    ) -> Result<Arc<dyn ConsoleVoiceSession>, VoiceError> {
        let slot = self.request_slot(principal, request).await?;
        let state = slot.state.lock().await;
        if state.cancelled || state.closed {
            return Err(VoiceError::Closed);
        }
        state.session.clone().ok_or(VoiceError::Busy)
    }

    pub(crate) async fn answer_received(
        &self,
        principal: &str,
        request: VoiceRequest,
        channel: &str,
    ) -> Result<(), VoiceError> {
        self.owned_session(principal, &request)
            .await?
            .answer_received(channel)
            .await?;
        let slot = self.request_slot(principal, &request).await?;
        let mut state = slot.state.lock().await;
        if state.cancelled || state.closed {
            return Err(VoiceError::Closed);
        }
        if !state.activated {
            state.activated = true;
            state.last_activity = Some(tokio::time::Instant::now());
            state.setup_deadline = None;
            slot.changed.notify_waiters();
        }
        Ok(())
    }

    pub(crate) async fn replacement_required(
        &self,
        principal: &str,
        request: VoiceRequest,
    ) -> Result<serde_json::Value, VoiceError> {
        self.owned_session(principal, &request)
            .await?
            .replacement_required()
            .await
    }

    pub(crate) async fn dispatch_channel(
        &self,
        principal: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, VoiceError> {
        let identity = params
            .get("identity")
            .and_then(serde_json::Value::as_str)
            .ok_or(VoiceError::InvalidRequest)?;
        let channel = params
            .get("channel_id")
            .and_then(serde_json::Value::as_str)
            .ok_or(VoiceError::InvalidRequest)?;
        let requests = self.requests.lock().await;
        let slots = requests
            .iter()
            .filter(|((owner, _), slot)| owner == principal && slot.identity == identity)
            .map(|(_, slot)| Arc::clone(slot))
            .collect::<Vec<_>>();
        drop(requests);
        for slot in slots {
            let state = slot.state.lock().await;
            if !state.cancelled
                && let Some(session) = state.session.as_ref()
                && session.pending().channel_id == channel
            {
                let session = Arc::clone(session);
                drop(state);
                return session.dispatch(method, params).await;
            }
        }
        Err(VoiceError::RequestConflict)
    }

    async fn request_slot(
        &self,
        principal: &str,
        request: &VoiceRequest,
    ) -> Result<Arc<RequestSlot>, VoiceError> {
        request.validate()?;
        if principal.trim().is_empty() {
            return Err(VoiceError::Unauthorized);
        }
        let requests = self.requests.lock().await;
        let slot = requests
            .get(&(principal.to_string(), request.request_id.clone()))
            .ok_or(VoiceError::RequestConflict)?;
        if slot.identity != request.identity {
            return Err(VoiceError::RequestConflict);
        }
        Ok(Arc::clone(slot))
    }

    // No production host is installed until the upstream summary,
    // existing-member execution and authenticated readiness seams are composed.
    #[allow(dead_code)]
    pub(crate) fn new(host: Arc<dyn ConsoleVoiceHost>) -> Self {
        Self {
            host: Some(host),
            requests: Arc::default(),
            stopped: Arc::default(),
        }
    }
}

impl ConsoleVoiceController {
    pub(crate) async fn open(
        &self,
        principal: &str,
        request: VoiceRequest,
    ) -> Result<PendingLiveChannelHandle, VoiceError> {
        request.validate()?;
        if principal.trim().is_empty() {
            return Err(VoiceError::Unauthorized);
        }
        let host = self.host.as_ref().ok_or(VoiceError::Unavailable)?;
        if !host.ready(principal, &request.identity).await? {
            return Err(VoiceError::Unavailable);
        }
        let key = (principal.to_string(), request.request_id.clone());
        let mut requests = self.requests.lock().await;
        if self.stopped.load(Ordering::SeqCst) {
            return Err(VoiceError::Unavailable);
        }
        let slot = if let Some(slot) = requests.get(&key) {
            if slot.identity != request.identity {
                return Err(VoiceError::RequestConflict);
            }
            Arc::clone(slot)
        } else {
            if requests.len() >= MAX_REQUESTS {
                return Err(VoiceError::RequestCapacity);
            }
            for ((owner, _), slot) in requests.iter() {
                let state = slot.state.lock().await;
                if owner == principal && !state.closed && state.open_error.is_none() {
                    return Err(VoiceError::Busy);
                }
            }
            let slot = Arc::new(RequestSlot::new(
                request.identity.clone(),
                RequestState {
                    opening: true,
                    ..RequestState::default()
                },
            ));
            requests.insert(key, Arc::clone(&slot));
            slot.supervise();
            let host = Arc::clone(host);
            let owner = principal.to_string();
            let pending = Arc::clone(&slot);
            tokio::spawn(async move {
                let result = host.open(&owner, &request.identity).await;
                let mut state = pending.state.lock().await;
                state.opening = false;
                match result {
                    Ok(session) => {
                        state.session = Some(session);
                        state.setup_deadline =
                            Some(tokio::time::Instant::now() + PENDING_SETUP_LIMIT);
                    }
                    Err(error) => {
                        state.open_error = Some(error);
                        state.closed = true;
                    }
                }
                pending.changed.notify_waiters();
            });
            slot
        };
        drop(requests);
        slot.result().await
    }

    pub(crate) async fn close(
        &self,
        principal: &str,
        request: VoiceRequest,
    ) -> Result<(), VoiceError> {
        request.validate()?;
        if principal.trim().is_empty() {
            return Err(VoiceError::Unauthorized);
        }
        let key = (principal.to_string(), request.request_id);
        let mut requests = self.requests.lock().await;
        let slot = if let Some(slot) = requests.get(&key) {
            if slot.identity != request.identity {
                return Err(VoiceError::RequestConflict);
            }
            Arc::clone(slot)
        } else {
            if requests.len() >= MAX_REQUESTS {
                return Err(VoiceError::RequestCapacity);
            }
            // Retain cancellation even when close wins the race with open.
            // Tombstones are never evicted: capacity refuses new requests
            // instead of allowing a delayed request to resurrect old work.
            let slot = Arc::new(RequestSlot::new(
                request.identity,
                RequestState {
                    cancelled: true,
                    closed: true,
                    ..RequestState::default()
                },
            ));
            requests.insert(key, Arc::clone(&slot));
            slot
        };
        drop(requests);
        slot.cancel().await;
        tokio::time::timeout(CLOSE_WAIT, slot.wait_closed())
            .await
            .map_err(|_| VoiceError::Busy)?
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use meerkat_contracts::{
        WireLiveChannelCapabilities, WireLiveContinuityMode, WireLiveTransportBootstrap,
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Semaphore;

    struct Session {
        close_calls: AtomicUsize,
        fail_close: AtomicBool,
    }

    #[async_trait]
    impl ConsoleVoiceSession for Session {
        fn pending(&self) -> PendingLiveChannelHandle {
            PendingLiveChannelHandle {
                channel_id: "test-channel".to_string(),
                target_identity: "agent-a".to_string(),
                execution_mode: crate::live_contracts::LiveExecutionMode::ClientContext,
                pending_receipt: "opaque-pending".to_string(),
                transport: WireLiveTransportBootstrap::Webrtc {
                    token: "opaque-token".to_string(),
                    answer_method: "live/webrtc/answer".to_string(),
                    http_url: None,
                },
                capabilities: WireLiveChannelCapabilities {
                    audio_in: true,
                    audio_out: true,
                    text_in: false,
                    text_out: false,
                    image_in: false,
                    video_in: false,
                    transcript_supported: true,
                    barge_in_supported: true,
                    provider_native_resume: false,
                },
                continuity: WireLiveContinuityMode::TranscriptOnly,
            }
        }

        async fn close(&self) -> Result<(), VoiceError> {
            self.close_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_close.load(Ordering::SeqCst) {
                Err(VoiceError::HostFailed)
            } else {
                Ok(())
            }
        }

        async fn answer_received(&self, channel: &str) -> Result<(), VoiceError> {
            if channel != self.pending().channel_id {
                return Err(VoiceError::RequestConflict);
            }
            Ok(())
        }
    }

    struct Host {
        ready: AtomicBool,
        checked_identities: Mutex<Vec<String>>,
        opens: AtomicUsize,
        started: Notify,
        permit: Semaphore,
        session: Arc<Session>,
    }

    impl Host {
        fn new(ready: bool, blocked: bool) -> Arc<Self> {
            Arc::new(Self {
                ready: AtomicBool::new(ready),
                checked_identities: Mutex::new(Vec::new()),
                opens: AtomicUsize::new(0),
                started: Notify::new(),
                permit: Semaphore::new(usize::from(!blocked)),
                session: Arc::new(Session {
                    close_calls: AtomicUsize::new(0),
                    fail_close: AtomicBool::new(false),
                }),
            })
        }
    }

    #[async_trait]
    impl ConsoleVoiceHost for Host {
        async fn ready(&self, _principal: &str, identity: &str) -> Result<bool, VoiceError> {
            self.checked_identities
                .lock()
                .await
                .push(identity.to_string());
            Ok(self.ready.load(Ordering::SeqCst))
        }

        async fn open(
            &self,
            _principal: &str,
            _identity: &str,
        ) -> Result<Arc<dyn ConsoleVoiceSession>, VoiceError> {
            self.opens.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.permit.acquire().await.expect("open permit").forget();
            Ok(Arc::clone(&self.session) as Arc<dyn ConsoleVoiceSession>)
        }
    }

    fn request() -> VoiceRequest {
        VoiceRequest {
            identity: "agent-a".to_string(),
            request_id: "request-a".to_string(),
        }
    }

    fn before_silence_expiry() -> Duration {
        SILENCE_LIMIT
            .checked_sub(Duration::from_secs(1))
            .expect("silence limit exceeds one second")
    }

    #[tokio::test]
    async fn configuration_discovery_does_not_probe_and_readiness_targets_one_identity() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        assert!(controller.configured());
        assert!(host.checked_identities.lock().await.is_empty());
        assert!(
            controller
                .ready("alice", "agent-a")
                .await
                .expect("target readiness")
        );
        assert_eq!(
            *host.checked_identities.lock().await,
            vec!["agent-a".to_string()]
        );
        assert_eq!(host.opens.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn missing_host_and_missing_auth_never_open() {
        assert_eq!(
            ConsoleVoiceController::default()
                .open("alice", request())
                .await,
            Err(VoiceError::Unavailable)
        );
        let host = Host::new(false, false);
        let controller = ConsoleVoiceController::new(host.clone());
        assert_eq!(
            controller.open("alice", request()).await,
            Err(VoiceError::Unavailable)
        );
        host.ready.store(true, Ordering::SeqCst);
        assert_eq!(
            controller.open("", request()).await,
            Err(VoiceError::Unauthorized)
        );
        assert_eq!(host.opens.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn repeated_open_is_idempotent_and_request_cannot_retarget() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        let pending = controller.open("alice", request()).await.expect("open");
        assert_eq!(
            controller.open("alice", request()).await.expect("retry"),
            pending
        );
        let mut retargeted = request();
        retargeted.identity = "agent-b".to_string();
        assert_eq!(
            controller.open("alice", retargeted.clone()).await,
            Err(VoiceError::RequestConflict)
        );
        assert_eq!(
            controller.close("alice", retargeted).await,
            Err(VoiceError::RequestConflict)
        );
        assert_eq!(host.opens.load(Ordering::SeqCst), 1);
        controller.close("alice", request()).await.expect("close");
    }

    #[tokio::test]
    async fn close_before_open_retains_cancellation_fence() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        controller.close("alice", request()).await.expect("fence");
        assert_eq!(
            controller.open("alice", request()).await,
            Err(VoiceError::Cancelled)
        );
        assert_eq!(host.opens.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn late_open_is_closed_even_after_both_http_waiters_disconnect() {
        let host = Host::new(true, true);
        let controller = ConsoleVoiceController::new(host.clone());
        let opener = controller.clone();
        let open = tokio::spawn(async move { opener.open("alice", request()).await });
        host.started.notified().await;
        open.abort();
        let slot = controller
            .requests
            .lock()
            .await
            .get(&("alice".to_string(), "request-a".to_string()))
            .expect("slot")
            .clone();
        slot.cancel().await;
        let closer = controller.clone();
        let close = tokio::spawn(async move { closer.close("alice", request()).await });
        close.abort();
        assert!(!slot.state.lock().await.closed);
        host.permit.add_permits(1);
        tokio::time::timeout(Duration::from_secs(1), slot.wait_closed())
            .await
            .expect("cleanup completes")
            .expect("closed");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            controller.open("alice", request()).await,
            Err(VoiceError::Cancelled)
        );
    }

    #[tokio::test]
    async fn text_cannot_be_reported_as_voice_activity() {
        assert!(
            serde_json::from_value::<VoiceActivity>(serde_json::json!({
                "identity":"agent-a", "request_id":"request-a"
            }))
            .is_ok()
        );
        assert!(
            serde_json::from_value::<VoiceActivity>(serde_json::json!({
                "identity":"agent-a", "request_id":"request-a", "kind":"text"
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn readiness_does_not_open_and_remains_available_for_an_active_voice_target() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        assert!(
            controller
                .ready("alice", "agent-a")
                .await
                .expect("readiness")
        );
        assert_eq!(host.opens.load(Ordering::SeqCst), 0);
        controller.open("alice", request()).await.expect("open");
        controller
            .answer_received("alice", request(), "test-channel")
            .await
            .expect("activation");
        assert!(
            controller
                .request_slot("alice", &request())
                .await
                .expect("slot")
                .state
                .lock()
                .await
                .activated
        );
        assert!(
            controller
                .ready("alice", "agent-a")
                .await
                .expect("active readiness")
        );
        assert_eq!(host.opens.load(Ordering::SeqCst), 1);
        host.ready.store(false, Ordering::SeqCst);
        assert!(
            !controller
                .ready("alice", "agent-a")
                .await
                .expect("revoked readiness")
        );
        assert_eq!(
            controller.open("alice", request()).await,
            Err(VoiceError::Unavailable)
        );
        assert_eq!(
            host.opens.load(Ordering::SeqCst),
            1,
            "active readiness must not open a second channel"
        );
        controller.close("alice", request()).await.expect("close");
        assert_eq!(
            controller.replacement_required("alice", request()).await,
            Err(VoiceError::Closed),
        );
    }

    #[tokio::test(start_paused = true)]
    async fn expensive_open_does_not_consume_the_voice_silence_window() {
        let host = Host::new(true, true);
        let controller = ConsoleVoiceController::new(host.clone());
        let opener = controller.clone();
        let open = tokio::spawn(async move { opener.open("alice", request()).await });
        host.started.notified().await;
        let slot = controller
            .request_slot("alice", &request())
            .await
            .expect("slot");
        tokio::time::advance(SILENCE_LIMIT + Duration::from_secs(1)).await;
        assert!(
            !slot.state.lock().await.cancelled,
            "setup is not voice silence"
        );
        host.permit.add_permits(1);
        open.await.expect("open task").expect("pending handle");
        assert!(slot.state.lock().await.last_activity.is_none());
        controller
            .answer_received("alice", request(), "test-channel")
            .await
            .expect("activation");
        tokio::time::advance(before_silence_expiry()).await;
        assert!(!slot.state.lock().await.cancelled);
        controller.close("alice", request()).await.expect("close");
    }

    #[tokio::test(start_paused = true)]
    async fn abandoned_pending_setup_has_a_separate_cleanup_deadline() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        controller
            .open("alice", request())
            .await
            .expect("pending handle");
        let slot = controller
            .request_slot("alice", &request())
            .await
            .expect("slot");
        assert!(slot.state.lock().await.last_activity.is_none());
        tokio::time::advance(PENDING_SETUP_LIMIT).await;
        slot.wait_closed().await.expect("abandoned setup cleaned");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn activation_starts_silence_window_but_replacement_ack_does_not_extend_it() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        controller.open("alice", request()).await.expect("open");
        tokio::time::advance(Duration::from_secs(30)).await;
        controller
            .answer_received("alice", request(), "test-channel")
            .await
            .expect("initial activation");
        tokio::time::advance(before_silence_expiry()).await;
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 0);
        controller
            .answer_received("alice", request(), "test-channel")
            .await
            .expect("repeated activation");
        tokio::time::advance(Duration::from_secs(1)).await;
        controller
            .request_slot("alice", &request())
            .await
            .expect("slot")
            .wait_closed()
            .await
            .expect("closed");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn server_silence_watchdog_uses_only_explicit_audio_activity() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        controller.open("alice", request()).await.expect("open");
        controller
            .answer_received("alice", request(), "test-channel")
            .await
            .expect("activation acknowledgement");
        let slot = controller
            .request_slot("alice", &request())
            .await
            .expect("slot");
        tokio::time::advance(before_silence_expiry()).await;
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 0);
        controller
            .note_activity(
                "alice",
                VoiceActivity {
                    identity: request().identity,
                    request_id: request().request_id,
                },
            )
            .await
            .expect("actual model audio");
        tokio::time::advance(before_silence_expiry()).await;
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 0);
        tokio::time::advance(Duration::from_secs(1)).await;
        slot.wait_closed()
            .await
            .expect("silence closes through host");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn close_is_principal_scoped_and_survives_readiness_revocation() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        controller.open("alice", request()).await.expect("open");
        controller
            .close("bob", request())
            .await
            .expect("other principal tombstone");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 0);
        host.ready.store(false, Ordering::SeqCst);
        controller
            .close("alice", request())
            .await
            .expect("owner close");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn close_failure_remains_retryable_and_blocks_another_open() {
        let host = Host::new(true, false);
        let controller = ConsoleVoiceController::new(host.clone());
        controller.open("alice", request()).await.expect("open");
        host.session.fail_close.store(true, Ordering::SeqCst);
        assert_eq!(
            controller.close("alice", request()).await,
            Err(VoiceError::HostFailed)
        );
        let mut next = request();
        next.request_id = "next-request".to_string();
        assert_eq!(controller.open("alice", next).await, Err(VoiceError::Busy));
        host.session.fail_close.store(false, Ordering::SeqCst);
        controller
            .close("alice", request())
            .await
            .expect("retry closes");
        controller
            .close("alice", request())
            .await
            .expect("close idempotent");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn close_timeout_never_claims_closed_and_eventual_cleanup_is_retained() {
        let host = Host::new(true, true);
        let controller = ConsoleVoiceController::new(host.clone());
        let opener = controller.clone();
        let open = tokio::spawn(async move { opener.open("alice", request()).await });
        host.started.notified().await;
        assert_eq!(
            controller.close("alice", request()).await,
            Err(VoiceError::Busy)
        );
        assert_eq!(open.await.expect("open waiter"), Err(VoiceError::Cancelled));
        host.permit.add_permits(1);
        controller
            .close("alice", request())
            .await
            .expect("eventual close");
        assert_eq!(host.session.close_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn voice_request_rejects_authority_fields_and_ambiguous_identifiers() {
        assert!(
            serde_json::from_value::<VoiceRequest>(serde_json::json!({
                "identity": "agent-a", "request_id": "r", "principal": "administrator"
            }))
            .is_err()
        );
        for identity in ["", " agent-a", "agent-a ", "rt:other"] {
            let mut invalid = request();
            invalid.identity = identity.to_string();
            assert_eq!(invalid.validate(), Err(VoiceError::InvalidRequest));
        }
    }
}
