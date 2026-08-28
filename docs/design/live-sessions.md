# Live (realtime) member sessions through the gateway

Status: experimental ClientContext integration candidate (2026-08-26).
Consumer shape: a LAN client (HomeCore's robot or a satellite process) opens
a realtime audio/text channel to a Mob member identity. The GPT Live endpoint
owns conversation and transport only. When GPT Live initiates client
delegation, Meerkat admits the exact canonical final transcript and runs work
through a separate ordinary durable executor fork. Tools and callbacks belong
to that executor, never to the voice endpoint. The canonical conversation
persists through Meerkat's normal transcript authority.

## Upstream shape (meerkat 0.7.25, surveyed)

- `meerkat-live` is deliberately embeddable: `LiveAdapterHost` (the
  transport-side orchestrator) + an axum WS router
  (`live_ws_router(Arc<LiveWsState>)`, path `/live/ws`) that mounts on any
  existing `Router`. It has NO meerkat-runtime dependency; canonical
  semantics arrive through two injected traits: `LiveProjectionSink` and
  `LiveToolDispatcher`. WebRTC optional behind the `webrtc` feature.
- Experimental GPT Live canonical projection is shared by Meerkat's generic
  `ServiceLiveProjection<B>`, which implements `LiveProjectionSink`,
  `LiveChannelCloseFeedback`, `LiveChannelStatusFeedback`, and
  `LiveWsTokenAuthority`. MobKit composes this facade directly and does not
  port experimental transcript, playback, or machine-authority sequences.
  Published Meerkat 0.8.26 still exposes only its Factory-bound projection,
  so non-experimental builds retain MobKit's preexisting ordinary websocket
  projection behind an exclusive compatibility cfg. The two paths cannot be
  compiled into the same build.
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
- Ordinary live compatibility tool dispatch:
  `LiveToolDispatcher::dispatch_live_tool_call(session_id, call)` ->
  `dispatch_external_tool_call` -> the session agent's normal external-tool
  dispatch. The gateway's `CallbackToolDispatcher`, composed recorder tools,
  and gating apply to those ordinary live turns unchanged. This does not apply
  to experimental GPT Live ClientContext, whose voice endpoint has no direct
  tool dispatcher and uses a distinct durable executor fork.
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

1. **Projection ownership** - experimental builds compose the shared Meerkat
   `ServiceLiveProjection<B: SessionAgentBuilder>` facade with the gateway's
   existing persistent service and `MeerkatMachine`. It is the single owner
   of experimental canonical transcript projection, assistant-output target
   admission/bind, playback completion, truncation and Unmeasured settlement,
   close/status feedback, and token authority. Stock 0.8.26 builds compile
   only the preexisting ordinary `GatewayLiveProjectionSink<B>` compatibility
   path until the generic facade is available in the minimum published
   dependency.
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
   profile field — profiles are upstream schema.) For members whose text
   PROVIDER differs too (HomeCore: Anthropic text profiles opening the
   OpenAI realtime lane), `mobkit/live/open` also accepts a strict
   optional `provider` paired with `model`: an unrecognized provider name
   is a typed invalid-params error (never a silent fallthrough), the
   (provider, model) pair is applied to the channel identity before the
   B19 precheck and machine admission, and when the selection differs
   from the member's inherited provider the inherited provider-specific
   auth binding is cleared so the selected provider's configured default
   credential resolution applies. Omitting `provider` keeps the previous
   behavior byte-identical. Both `model` and `provider` are CHANNEL-scoped:
   they mutate only the per-open `RealtimeSessionOpenConfig` projection
   (the member's durable identity is read via
   `live_session_llm_identity`, never written on this path), so channel
   close reverts by construction and `live/refresh` re-projects from the
   durable session.
7. **Gating**: `runtime_options.live = true | {ws: true}` opt-in on
   `mobkit/init` (default OFF). ABAC: live methods map to `agent.send` on
   the target member (console surface); stdin surface is host-trusted as
   usual.

## Experimental channel-scoped execution

The experimental surface is stricter than ordinary live compatibility.
It accepts only an identity-first durable member target. Raw `session_id`,
runtime `member_id`, mixed target forms, and stale aliases cannot acquire the
experimental capability. The nested `execution_identity` request is versioned
and strict. The caller does not select a provider-native delegation mode or
provide a Responses model, bridge instructions, or tool declaration.

The catalog-qualified shipping mode is `client_context`, backed by
`live.execution.client_context.v1`. It configures `delegation.type = "client"`
and sends no provider tools, Responses model, or Responses configuration. The
voice model decides when to delegate through GPT Live's defined client
delegation lifecycle. Meerkat then owns the canonical-transcript join,
durable executor fork, bounded result, and exact
`delegation.context.append` acknowledgement.

`function_bridge` remains dormant, independently gated vocabulary. Current
builds neither advertise nor select it because direct probes have not observed
the raw Responses function-call carrier or settlement lifecycle. It never
falls back silently to ClientContext.

Experimental open returns `PendingLiveChannelHandle`, not an active channel.
The pending receipt permits only playback-owner registration, status, WebRTC
answer under the resulting readiness receipt, and close. Activation mints a
distinct `ActiveLiveChannelHandle` with an opaque activation receipt. Refresh,
input, commit, interruption, truncation, replacement, and playback settlement
require that exact current active receipt. Playback-owner loss revokes active
authority before the provider can accept more effects.

The Python and TypeScript high-level connect methods install the gated media
owner, register readiness, answer, wait for generated activation, and only
then release media and return the active handle. Stock crates.io builds remain
portable and advertise none of these experimental capability atoms.

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
committed images. The shared Meerkat projection forwards the transcript apply outcome
(0.7.27 API) so the host synthesizes the redacted image receipt only after
durable reducer application. Only `gpt-realtime-2` accepts image input in
the shipped catalog (capabilities carry `image_in`). As of meerkat 0.7.28
the catalog default model is `gpt-5.6-sol` (GPT-5.6 Sol/Terra/Luna added;
explicit GPT-5.5 pins stay honored) — realtime capability is unchanged.

## Field-reported additions (mobkit 0.7.32)

- **Host-trusted voice profiles**: `mobkit/live/open` rejects caller-supplied
  instructions, mode, model, provider, tools, Responses configuration, and
  capability claims. A named profile selects host-owned session instructions;
  it cannot introduce callback or direct-effect authority. The profile and
  qualified ClientContext mode are not configurable through the untrusted open
  request.
- **Seed clamp (upstream ask 30 STOPGAP)**: providers cap live instructions
  at 65,536 tokens, so long member transcripts overflow the projected seed
  at open. `runtime_options.live.seed_max_chars` (object form) sets a
  gateway-wide serialized-char budget; per-open `seed_max_chars` overrides
  it. Whole canonical conversation messages drop oldest-first. System and
  SystemNotice authority text is excluded from experimental provider
  commentary entirely. Remove the clamp when Meerkat ships a machine-owned
  seed-window projection (ask 30).
- **`mobkit/live/truncate`** (was deliberately unported in v1): barge-in
  cleanup — truncate an assistant item at the client-tracked playback
  cursor. Same machine-authority choreography as the sibling command
  handlers; SDK conveniences `live_truncate` / `liveTruncate`.

## What ordinary live compatibility deliberately does not do

- WebRTC remains absent from the ordinary compatibility path. Experimental
  channel-scoped execution uses the separately gated WebRTC surface.
- Provider-native resume (upstream returns TranscriptOnly anyway).
- Console UI affordance (phase 2, with the SDK methods).
- A second listener/port: the WS mounts on the existing gateway HTTP app.

## Separate generic durable delegation

The ordinary agent-facing Mob tool roster also exposes `fork_off`. It is a
provider-neutral durable fork-plus-bounded-run operation for tool-capable
agents. It is not a GPT Live tool, imports no live protocol authority, and is
not required for voice to function. The experimental voice coordinator and
`fork_off` both reuse Meerkat's existing durable fork primitive through their
own typed callers.

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
