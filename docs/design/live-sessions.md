# Live (realtime) member sessions through the gateway

Status: in progress on `feat/live-sessions` (2026-07-09). Target: mobkit
0.7.31+. Consumer shape: a LAN client (HomeCore's robot, a satellite
process) opens a realtime audio/text channel to a MOB MEMBER (identity
target) and converses; tool calls flow through the member's normal tool
surface (callback bridge + gating); the conversation persists into the
member's durable transcript.

## Upstream shape (meerkat 0.7.25, surveyed)

- `meerkat-live` is deliberately embeddable: `LiveAdapterHost` (the
  transport-side orchestrator) + an axum WS router
  (`live_ws_router(Arc<LiveWsState>)`, path `/live/ws`) that mounts on any
  existing `Router`. It has NO meerkat-runtime dependency; canonical
  semantics arrive through two injected traits: `LiveProjectionSink` and
  `LiveToolDispatcher`. WebRTC optional behind the `webrtc` feature.
- The reference composition lives in the `meerkat-rpc` BINARY crate and is
  NOT re-exported: `SessionServiceProjectionSink` (implements the sink +
  `LiveChannelCloseFeedback` + `LiveChannelStatusFeedback` +
  `LiveWsTokenAuthority`), `RuntimeLiveToolDispatcher`,
  `handlers/live.rs` (the `live/*` RPC methods), and
  `build_per_open_realtime_session_factory` (per-open credential
  resolution). mobkit must port this glue.
- Lifecycle authority is machine-owned: every open/close/status/token step
  requires a non-forgeable authority minted by `MeerkatMachine` live
  methods (`resolve_live_open_admission`, `resolve_live_close_result`,
  `record_live_websocket_token_issued`,
  `resolve_live_websocket_token_admission`, ...). All present in published
  meerkat-runtime 0.7.25 under the `live` feature (mobkit already enables
  it).
- Session-scoped seams the sink needs are inherent methods on
  `PersistentSessionService<B>`: `append_external_user_content`,
  `append_external_assistant_output`, `append_realtime_transcript_event`,
  `dispatch_external_tool_call`, `record_live_terminal_error`,
  `record_live_output_audio_degraded`. Realtime transcript events commit
  into the ONE canonical Session history through the same append-only save
  path as normal turns (so the Bug B-2 rollback fix in
  `ContinuitySessionStoreAdapter` covers live turns too).
- Tool dispatch: `LiveToolDispatcher::dispatch_live_tool_call(session_id,
  call)` → `dispatch_external_tool_call` → the session agent's NORMAL
  external-tool dispatch. The gateway's `CallbackToolDispatcher` (and
  composed recorder tools, and gating) apply to live turns unchanged. No
  separate tool registry.
- Tokens: single-use, 60s TTL, pinned to (token, channel). Bootstrap
  returned by open: `{channel_id, transport: {type:"websocket", url,
  token}, capabilities, continuity}`. Continuity is TranscriptOnly in
  practice (no provider-native resume ships).
- Credentials resolve PER OPEN via
  `AgentFactory::build_openai_realtime_session_factory(config_source)`
  where `RealtimeCurrentConfigSource` is just `async fn current_config() ->
  Config`. Facade features required: `live` + `openai-realtime`. Only
  `gpt-realtime-2` (OpenAI) is realtime-capable in the shipped catalog.

## mobkit design

New module `meerkat-mobkit/src/live_wiring.rs`:

1. **`GatewayLiveProjectionSink<B: SessionAgentBuilder>`** — port of
   meerkat-rpc's `SessionServiceProjectionSink`, holding
   `Arc<PersistentSessionService<B>>` + `Arc<MeerkatMachine>` (the
   gateway's existing `adapter` instance — one machine per process, same
   one wired for image generation and the schedule host). Implements
   `LiveProjectionSink`, `LiveChannelCloseFeedback`,
   `LiveChannelStatusFeedback`, `LiveWsTokenAuthority`. Keeps the
   pending-turn buffering semantics verbatim (per-(session, response_id)
   display-text finals drained at TurnCompleted; spoken-transcript lane
   commits via the realtime staging materializer; R6: mismatched
   response_id never flushes another turn's transcript; fail-closed
   delta identity via `LiveTranscriptIdentity::require_delta_identity`).
2. **`GatewayLiveToolDispatcher<B>`** — `dispatch_external_tool_call` on
   the service.
3. **`GatewayLiveContext`** — `{host: Arc<LiveAdapterHost>, ws_state:
   Arc<LiveWsState>, session_factory: Arc<dyn RealtimeSessionFactory>,
   ws_base_url}` built by `attach_live(...)` in the gateway (persistent
   mode only — live needs the runtime-backed service). The gateway merges
   `meerkat_live::live_ws_router(ws_state)` onto the reference app router,
   so the live WS shares the existing HTTP listener/port (HomeCore's
   `app.py` proxy or direct LAN access both work).
4. **Credential source**: `EnvRealtimeConfigSource` implementing
   `RealtimeCurrentConfigSource` by returning the gateway's effective
   `Config` (same `Config::default()` the agent builds use). Per-open
   resolution then rides the session identity's auth binding or the
   provider default (env `OPENAI_API_KEY`), matching text-model behavior.
   Embedders with real config stores can swap the source later.
5. **RPC surface** (unified stdin + console): `mobkit/live/open`,
   `mobkit/live/status`, `mobkit/live/close`, `mobkit/live/refresh`,
   `mobkit/live/send_input`, `mobkit/live/commit_input`,
   `mobkit/live/interrupt`, `mobkit/live/truncate`. Params accept an
   IDENTITY TARGET —
   `{identity: "reachy"}` or `{member_id}` or raw `{session_id}` —
   resolved via `resolve_bridge_session_id` + the roster `agent_identity`
   label fallback (the same canonicalization class as
   `/agents/{id}/events` and `cross_mob/peer_info`). Handlers are ports of
   `meerkat-rpc/handlers/live.rs` against `GatewayLiveContext`. Methods
   answer `-32050 live_unavailable` when the gateway has no live context
   (ephemeral mode, or feature disabled).
6. **Realtime model selection (v1)**: the member session's model decides
   (profile `model = "gpt-realtime-2"` for a voice-first member). For
   members whose text model differs, `mobkit/live/open` accepts an
   optional `model` override forwarded into `RealtimeSessionOpenConfig`;
   a per-profile `realtime_model` map can ride `runtime_options.live` in a
   follow-up once field usage settles. (Deliberately NOT a mob.toml
   profile field — profiles are upstream schema.)
7. **Gating**: `runtime_options.live = true | {ws: true}` opt-in on
   `mobkit/init` (default OFF). ABAC: live methods map to `agent.send` on
   the target member (console surface); stdin surface is host-trusted as
   usual.

## Images (meerkat 0.7.27, mobkit 0.7.32)

Still-image input rides the SAME transport and RPC surface: the wire chunk
`{kind: "image", idempotency_key, mime, data}` flows through
`mobkit/live/send_input` unchanged (the handler deserializes
`LiveInputChunkWire`, which gained the exhaustive `Image` variant), and the
SDKs add `live_send_input_image` / `liveSendInputImage` conveniences
mirroring meerkat's SDK signatures. `idempotency_key` is caller-stable
within the session — retries are exact-retry deduplicated by the runtime's
user-content identity lane, which also rides the open config
(`user_content_identities` / `user_content_tombstones` /
`transcript_rewrite_generation`) so reopened channels do not replay
committed images. The projection sink forwards the transcript apply outcome
(0.7.27 API) so the host synthesizes the redacted image receipt only after
durable reducer application. Only `gpt-realtime-2` accepts image input in
the shipped catalog (capabilities carry `image_in`).

## Field-reported additions (mobkit 0.7.32)

- **Per-open instruction overlay**: `mobkit/live/open` accepts
  `instructions` (string or array of strings). The overlay rides the
  runtime system-context lane of the open projection
  (`with_runtime_system_context`), NOT the typed `system_prompt` — the
  prompt field is the projection of durable prompt truth (R10) and must not
  be contaminated by per-open ephemera. The overlay never persists into the
  durable transcript and is dropped by `live/refresh` (which re-projects
  from the durable session) and on reopen.
- **Seed clamp (upstream ask 30 STOPGAP)**: providers cap live instructions
  at 65,536 tokens, so long member transcripts overflow the projected seed
  at open. `runtime_options.live.seed_max_chars` (object form) sets a
  gateway-wide serialized-char budget; per-open `seed_max_chars` overrides
  it. Whole messages drop OLDEST-first; a projected root system message is
  never dropped. Remove when meerkat ships a machine-owned seed-window
  projection (ask 30).
- **`mobkit/live/truncate`** (was deliberately unported in v1): barge-in
  cleanup — truncate an assistant item at the client-tracked playback
  cursor. Same machine-authority choreography as the sibling command
  handlers; SDK conveniences `live_truncate` / `liveTruncate`.

## What we deliberately do NOT do in v1

- WebRTC (feature-gated upstream; WS covers the LAN robot).
- Provider-native resume (upstream returns TranscriptOnly anyway).
- Console UI affordance (phase 2, with the SDK methods).
- A second listener/port: the WS mounts on the existing gateway HTTP app.

## Test plan

- Sink unit tests: delta identity fail-closed, pending-turn drain keyed by
  response_id, terminal-error drain-all (ported assertions).
- A `FakeRealtimeSessionFactory` (scripted `RealtimeSessionEvent`s) driving
  an end-to-end open → observation → transcript-commit → close against a
  real persistent service + machine, asserting the member session's
  transcript contains the committed turn.
- Token admission: minted token admits once on the right channel; replay
  and cross-channel use rejected (machine-backed single-use).
- RPC: identity-target resolution parity (identity/member alias/session id
  spellings), live_unavailable off-state.
