import { test, vi } from "vitest";
import assert from "node:assert/strict";
import voiceContract from "../../../meerkat-mobkit/tests/fixtures/console_voice_v1.json";
import {
  createVoiceSession,
  queryVoiceAvailability,
  VOICE_CONNECT_TIMEOUT_MS,
  VOICE_RECOVERY_TIMEOUT_MS,
  VOICE_SILENCE_TIMEOUT_MS,
  VOICE_TEARDOWN_TIMEOUT_MS,
  VOICE_REPLACEMENT_POLL_INTERVAL_MS,
  VOICE_ACTIVITY_REPORT_INTERVAL_MS,
  type VoiceSessionEnvironment,
} from "./voice-session";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function flush() {
  for (let i = 0; i < 40; i++) await Promise.resolve();
}

class Clock {
  now = 0;
  id = 0;
  timers = new Map<number, { at: number; callback: () => void }>();
  setTimeout = (callback: () => void, milliseconds: number) => {
    const id = ++this.id;
    this.timers.set(id, { at: this.now + milliseconds, callback });
    return id as unknown as ReturnType<typeof setTimeout>;
  };
  clearTimeout = (id: ReturnType<typeof setTimeout>) => {
    this.timers.delete(id as unknown as number);
  };
  async advance(milliseconds: number) {
    await flush();
    const until = this.now + milliseconds;
    for (;;) {
      const next = [...this.timers].filter(([, timer]) => timer.at <= until)
        .sort((a, b) => a[1].at - b[1].at || a[0] - b[0])[0];
      if (!next) break;
      this.now = next[1].at;
      this.timers.delete(next[0]);
      next[1].callback();
      await flush();
    }
    this.now = until;
    await flush();
  }
}

class Track {
  kind = "audio";
  enabled = true;
  stopped = false;
  onended: (() => void) | null = null;
  stop() { this.stopped = true; }
  end() { this.onended?.(); }
}

class Stream {
  tracks: Track[];
  constructor(tracks = [new Track()]) { this.tracks = tracks; }
  getTracks() { return this.tracks; }
  getAudioTracks() { return this.tracks; }
  addTrack(track: Track) { this.tracks.push(track); }
}

class Node {
  connections: Node[] = [];
  disconnected = false;
  connect(node: Node) { this.connections.push(node); return node; }
  disconnect() { this.disconnected = true; }
}

class Analyser extends Node {
  fftSize = 2048;
  signal = 0;
  getFloatTimeDomainData(samples: Float32Array) { samples.fill(this.signal); }
}

class Gain extends Node {
  gain = { value: 1 };
}

class Context {
  state = "suspended";
  onstatechange: (() => void) | null = null;
  destination = new Node();
  analysers: Analyser[] = [];
  gains: Gain[] = [];
  sources: Node[] = [];
  resumed = false;
  resume() { this.resumed = true; this.state = "running"; return Promise.resolve(); }
  close() { this.state = "closed"; return Promise.resolve(); }
  createAnalyser() { const node = new Analyser(); this.analysers.push(node); return node; }
  createGain() { const node = new Gain(); this.gains.push(node); return node; }
  createMediaStreamSource() { const node = new Node(); this.sources.push(node); return node; }
}

class Channel {
  readyState = "connecting";
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  close() { this.readyState = "closed"; }
  emit(value: unknown) { this.onmessage?.({ data: JSON.stringify(value) }); }
}

class Peer {
  connectionState = "new";
  iceConnectionState = "new";
  channel = new Channel();
  ontrack: ((event: { track: Track }) => void) | null = null;
  onconnectionstatechange: (() => void) | null = null;
  oniceconnectionstatechange: (() => void) | null = null;
  localDescription: { type: string; sdp: string } | null = null;
  tracks: Track[] = [];
  offerPrepared = false;
  remoteTrack = new Track();
  onOffer: (() => void) | undefined;
  createDataChannel() { return this.channel; }
  addTrack(track: Track) { this.tracks.push(track); }
  createOffer() {
    assert.ok(this.ontrack, "output consumer installed before offer");
    assert.ok(this.channel.onmessage, "data consumer installed before offer");
    assert.ok(this.tracks.every((track) => !track.enabled), "microphone gated before offer");
    this.onOffer?.();
    this.offerPrepared = true;
    return Promise.resolve({ type: "offer", sdp: "offer-sdp" });
  }
  setLocalDescription(offer: { type: string; sdp: string }) {
    this.localDescription = offer;
    return Promise.resolve();
  }
  setRemoteDescription() {
    assert.ok(this.tracks.every((track) => !track.enabled), "mic remains gated while applying answer");
    this.ontrack?.({ track: this.remoteTrack });
    this.connectionState = "connected";
    this.iceConnectionState = "connected";
    this.channel.readyState = "open";
    return Promise.resolve();
  }
  close() { this.connectionState = "closed"; this.iceConnectionState = "closed"; }
  lose() { this.connectionState = "disconnected"; this.onconnectionstatechange?.(); }
}

const target = { identity: "agent-a", label: "Agent A" };
const other = { identity: "agent-b", label: "Agent B" };

function pending(identity = target.identity, id = "channel-a") {
  return {
    channel_id: id,
    target_identity: identity,
    execution_mode: "client_context",
    pending_receipt: "pending-receipt",
    transport: { transport: "webrtc", token: "opaque-bootstrap", answer_method: "live/webrtc/answer" },
    capabilities: {
      audio_in: true, audio_out: true, text_in: true, text_out: true, image_in: false,
      video_in: false, transcript_supported: true, barge_in_supported: true, provider_native_resume: false,
    },
    continuity: { mode: "fresh" },
  };
}

function replacement(reason = "canonical_context", handle: unknown = pending(target.identity, "recovery-channel")) {
  return { required: true, reason, replacement: handle, canonical_seed_cursor: 17 };
}

function harness() {
  const clock = new Clock();
  const streams: Stream[] = [];
  const peers: Peer[] = [];
  const contexts: Context[] = [];
  const calls: { method: string; params: Record<string, unknown> }[] = [];
  const pagehideCalls: Record<string, unknown>[] = [];
  let pagehide: (() => void) | undefined;
  let requestId = 0;
  let getMedia: (() => Promise<Stream>) | undefined;
  let onRpc: ((method: string, params: Record<string, unknown>) => Promise<unknown> | undefined) | undefined;
  let onContext: ((context: Context) => void) | undefined;
  let onPeer: ((peer: Peer) => void) | undefined;
  const env: VoiceSessionEnvironment = {
    voiceAvailable: () => Promise.resolve(true),
    createAudioContext: () => {
      const context = new Context();
      contexts.push(context);
      onContext?.(context);
      return context as unknown as AudioContext;
    },
    getUserMedia: () => {
      if (getMedia) return getMedia() as unknown as Promise<MediaStream>;
      const stream = new Stream();
      streams.push(stream);
      return Promise.resolve(stream as unknown as MediaStream);
    },
    createPeerConnection: () => {
      const peer = new Peer();
      peers.push(peer);
      onPeer?.(peer);
      return peer as unknown as RTCPeerConnection;
    },
    createMediaStream: (tracks) => new Stream((tracks ?? []) as unknown as Track[]) as unknown as MediaStream,
    createAudioElement: () => ({
      autoplay: false, muted: false, srcObject: null, play: () => Promise.resolve(), pause() {},
    }) as unknown as HTMLAudioElement,
    rpc: async (method, params) => {
      calls.push({ method, params });
      const overridden = onRpc?.(method, params);
      if (overridden) return overridden;
      if (method === "mobkit/console/voice/readiness") return { identity: params.identity, available: true };
      if (method === "mobkit/console/voice/replacement") return { required: false };
      if (method === "mobkit/console/voice/answer_received") return { accepted: true };
      if (method === "mobkit/console/voice/activity") return { accepted: true };
      if (method === "mobkit/console/voice/open") {
        return pending(params.identity as string, `channel-${params.identity}`);
      }
      if (method === "mobkit/console/voice/close") return { phase: "closed" };
      if (method === "mobkit/live/playback_owner/register") {
        return { channel_id: params.channel_id, readiness_receipt: "readiness-receipt" };
      }
      if (method === "live/webrtc/answer") return { answer_sdp: "answer-sdp" };
      if (method === "mobkit/live/status") {
        return {
          phase: "active",
          handle: {
            channel_id: params.channel_id,
            target_identity: params.identity,
            execution_mode: "client_context",
            activation_receipt: "activation-receipt",
          },
        };
      }
      throw new Error(`Unexpected RPC: ${method}`);
    },
    now: () => clock.now,
    randomId: () => `request-${++requestId}`,
    setTimeout: clock.setTimeout,
    clearTimeout: clock.clearTimeout,
    onPagehide: (listener) => { pagehide = listener; return () => { pagehide = undefined; }; },
    pagehideClose: (params) => { pagehideCalls.push(params); },
  };
  const controller = createVoiceSession("", env);
  return {
    controller, env, clock, calls, contexts, peers, streams, pagehideCalls,
    setMedia: (fn: typeof getMedia) => { getMedia = fn; },
    setRpc: (fn: typeof onRpc) => { onRpc = fn; },
    setContext: (fn: typeof onContext) => { onContext = fn; },
    setPeer: (fn: typeof onPeer) => { onPeer = fn; },
    pagehide: () => pagehide?.(),
  };
}

test("strict handshake installs output before offer, gates media until typed authority, and hides credentials", async () => {
  const h = harness();
  const status = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/live/status" ? status.promise : undefined);
  const start = h.controller.start(target);
  assert.equal(h.contexts[0].resumed, true, "AudioContext resumed synchronously in click gesture");
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.streams[0].tracks[0].enabled, false);
  assert.equal(h.contexts[0].gains[0].gain.value, 0);
  const statusCall = h.calls.find((call) => call.method === "mobkit/live/status")!;
  status.resolve({
    phase: "active", handle: {
      channel_id: statusCall.params.channel_id, target_identity: target.identity,
      execution_mode: "client_context", activation_receipt: "activation-secret",
    },
  });
  await start;
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.equal(h.streams[0].tracks[0].enabled, true);
  assert.equal(h.contexts[0].gains[0].gain.value, 1);
  assert.deepEqual(h.calls.map((call) => call.method), [
    "mobkit/console/voice/open", "mobkit/live/playback_owner/register",
    "live/webrtc/answer", "mobkit/console/voice/answer_received", "mobkit/live/status",
  ]);
  assert.doesNotMatch(JSON.stringify(h.controller.getSnapshot()), /secret|receipt|opaque-bootstrap|sdp/);
  await h.controller.close();
  assert.equal(h.clock.timers.size, 0);
});

test("pending-handle activation preserves target and mute choices without opening another channel", async () => {
  const h = harness();
  const status = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/live/status" ? status.promise : undefined);
  const start = h.controller.start(target);
  await flush();
  h.controller.toggleMicrophone();
  h.controller.toggleSpeaker();
  status.resolve({
    phase: "active",
    handle: {
      channel_id: `channel-${target.identity}`, target_identity: target.identity,
      execution_mode: "client_context", activation_receipt: "active-receipt",
    },
  });
  await start;
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.deepEqual(h.controller.getSnapshot().target, target);
  assert.equal(h.controller.getSnapshot().microphoneMuted, true);
  assert.equal(h.controller.getSnapshot().speakerMuted, true);
  assert.equal(h.streams[0].tracks[0].enabled, false);
  assert.equal(h.contexts[0].gains[0].gain.value, 0);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  await h.controller.close();
});

test("stable snapshot and voice target survive independent background text", async () => {
  const h = harness();
  h.setRpc((method) => method === "mobkit/console/send" ? Promise.resolve({ accepted: true }) : undefined);
  let changes = 0;
  const unsubscribe = h.controller.subscribe(() => { changes++; });
  await h.controller.start(target);
  const snapshot = h.controller.getSnapshot();
  const before = changes;
  await h.env.rpc("mobkit/console/send", { identity: other.identity, message: "Independent text" }, 5000);
  assert.equal(h.controller.getSnapshot(), snapshot);
  assert.equal(changes, before);
  assert.deepEqual(snapshot.target, target);
  unsubscribe();
  await h.controller.close();
  assert.equal(changes, before);
});

test("microphone and speaker mute independently, output waveform and activity continue while speaker-muted", async () => {
  const h = harness();
  await h.controller.start(target);
  const context = h.contexts[0];
  context.analysers[0].signal = 0.25;
  context.analysers[1].signal = 0.5;
  h.controller.toggleMicrophone();
  assert.equal(h.streams[0].tracks[0].enabled, false);
  assert.equal(context.gains[0].gain.value, 1);
  h.controller.toggleSpeaker();
  assert.equal(context.gains[0].gain.value, 0);
  assert.equal(h.peers[0].remoteTrack.enabled, true);
  const samples = new Float32Array(32);
  h.controller.sampleWaveform("speaker", samples);
  assert.equal(samples[0], 0.5);
  h.controller.sampleWaveform("microphone", samples);
  assert.equal(samples[0], 0);
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS + 1000);
  assert.equal(h.controller.getSnapshot().phase, "active", "model sound refreshes silence even when muted");
  h.controller.toggleMicrophone();
  assert.equal(h.streams[0].tracks[0].enabled, true);
  h.controller.toggleSpeaker();
  assert.equal(context.gains[0].gain.value, 1);
  await h.controller.close();
});

test("closes at exactly 900000ms of silence despite silent PCM and keepalives", async () => {
  const h = harness();
  await h.controller.start(target);
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - 1);
  h.peers[0].channel.emit({ type: "session.updated" });
  h.peers[0].channel.emit({ type: "response.output_audio.delta", delta: "AAAA" });
  h.peers[0].channel.emit({ type: "response.audio_transcript.delta", delta: "   " });
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.match(h.controller.getSnapshot().notice!, /15 minutes of silence/);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.peers[0].connectionState, "closed");
  assert.equal(h.contexts[0].state, "closed");
  assert.equal(h.clock.timers.size, 0);
});

test("independent text to either agent and model text or transcripts never refresh audio silence", async () => {
  const h = harness();
  h.setRpc((method) => method === "mobkit/console/send" ? Promise.resolve({ accepted: true }) : undefined);
  await h.controller.start(target);
  await h.clock.advance(800_000);
  for (const identity of [target.identity, other.identity]) {
    const result = await h.env.rpc("mobkit/console/send", { identity, message: "Still independently texting" }, 5000);
    assert.deepEqual(result, { accepted: true });
  }
  for (const type of [
    "response.text.delta", "response.output_text.delta",
    "response.audio_transcript.delta", "response.output_audio_transcript.delta",
  ]) h.peers[0].channel.emit({ type, delta: "Text without current audio" });
  h.peers[0].channel.emit({
    type: "conversation.item.input_audio_transcription.completed",
    transcript: "Delayed transcription does not mean current speech",
  });
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - 800_000 - 1);
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
});

test("authoritative microphone speech events refresh silence only for an unmuted microphone", async () => {
  const h = harness();
  await h.controller.start(target);
  await h.clock.advance(800_000);
  h.peers[0].channel.emit({ type: "input_audio_buffer.speech_started" });
  await h.clock.advance(800_000);
  assert.equal(h.controller.getSnapshot().phase, "active");
  h.peers[0].channel.emit({ type: "input_audio_buffer.speech_stopped" });
  h.controller.toggleMicrophone();
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - 1);
  h.peers[0].channel.emit({ type: "input_audio_buffer.speech_started" });
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
});

test("actual microphone activity refreshes timeout; muted input does not", async () => {
  const h = harness();
  await h.controller.start(target);
  h.contexts[0].analysers[0].signal = 0.2;
  await h.clock.advance(1000);
  h.controller.toggleMicrophone();
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - 1);
  h.peers[0].channel.emit({ type: "input_audio_buffer.speech_started" });
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
});

test("replacement stops local media immediately and waits for confirmed remote close before opening next agent", async () => {
  const h = harness();
  await h.controller.start(target);
  const closing = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/console/voice/close" ? closing.promise : undefined);
  const replacement = h.controller.start(other);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.peers[0].connectionState, "closed");
  await flush();
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  closing.resolve({ phase: "closed" });
  await replacement;
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.equal(h.controller.getSnapshot().target?.identity, other.identity);
  const closeIndex = h.calls.findIndex((call) => call.method.endsWith("/close"));
  const nextIndex = h.calls.findIndex((call) => call.method.endsWith("/open") && call.params.identity === other.identity);
  assert.ok(closeIndex < nextIndex);
  await h.controller.close();
});

test("fast start/start/close never opens stale requests and late microphone grants stop tracks", async () => {
  const h = harness();
  const media = deferred<Stream>();
  h.setMedia(() => media.promise);
  const first = h.controller.start(target);
  await flush();
  const second = h.controller.start(other);
  await flush();
  const close = h.controller.close();
  await Promise.all([first, second, close]);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  const stream = new Stream();
  media.resolve(stream);
  await flush();
  assert.equal(stream.tracks[0].stopped, true);
  assert.equal(stream.tracks[0].enabled, false);
  assert.equal(h.calls.length, 0);
  assert.ok(h.contexts.every((context) => context.state === "closed"));
});

test("close fences an in-flight open, and a late response cannot activate or overwrite a replacement", async () => {
  const h = harness();
  const opened = deferred<unknown>();
  h.setRpc((method, params) =>
    method.endsWith("/open") && params.identity === target.identity ? opened.promise : undefined);
  const first = h.controller.start(target);
  await flush();
  assert.equal(h.calls[0].method, "mobkit/console/voice/open");
  const second = h.controller.start(other);
  await Promise.all([first, second]);
  assert.equal(h.controller.getSnapshot().target?.identity, other.identity);
  assert.equal(h.controller.getSnapshot().phase, "active");
  const close = h.calls.find((call) => call.method.endsWith("/close"))!;
  assert.equal(close.params.request_id, h.calls[0].params.request_id);
  opened.resolve(pending());
  await flush();
  assert.equal(h.controller.getSnapshot().target?.identity, other.identity);
  assert.equal(h.peers.length, 1);
  await h.controller.close();
});

test("pending activation does not enable media and has a bounded deadline", async () => {
  const h = harness();
  h.setRpc((method) => method === "mobkit/live/status" ? Promise.resolve({ phase: "pending" }) : undefined);
  const start = h.controller.start(target);
  await flush();
  h.controller.toggleMicrophone();
  h.controller.toggleMicrophone();
  assert.equal(h.streams[0].tracks[0].enabled, false);
  await h.clock.advance(VOICE_CONNECT_TIMEOUT_MS);
  await start;
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.match(h.controller.getSnapshot().error!, /timed out/);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.contexts[0].state, "closed");
  assert.equal(h.clock.timers.size, 0);
});

test("permission denial, permission timeout, and late permission grants clean up without opening remote voice", async () => {
  const denied = harness();
  denied.setMedia(() => Promise.reject(Object.assign(new Error("denied"), { name: "NotAllowedError" })));
  await denied.controller.start(target);
  assert.equal(denied.controller.getSnapshot().phase, "error");
  assert.match(denied.controller.getSnapshot().error!, /permission was denied/);
  assert.equal(denied.calls.length, 0);
  assert.equal(denied.contexts[0].state, "closed");
  const h = harness();
  const media = deferred<Stream>();
  h.setMedia(() => media.promise);
  const start = h.controller.start(target);
  await flush();
  await h.clock.advance(VOICE_CONNECT_TIMEOUT_MS);
  await start;
  const stream = new Stream();
  media.resolve(stream);
  await flush();
  assert.equal(stream.tracks[0].stopped, true);
  assert.equal(h.calls.length, 0);
});

test("unconfirmed teardown fails closed and can be retried explicitly", async () => {
  const h = harness();
  await h.controller.start(target);
  h.setRpc((method) => method.endsWith("/close") ? new Promise(() => {}) : undefined);
  const close = h.controller.close();
  await flush();
  await h.clock.advance(VOICE_TEARDOWN_TIMEOUT_MS);
  await close;
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.match(h.controller.getSnapshot().error!, /not confirmed voice closure/);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  await h.controller.start(other);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  h.setRpc(undefined);
  await h.controller.close();
  await h.controller.start(other);
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.controller.close();
});

test("RTC, data channel, input track, and remote track loss close local and remote voice", async () => {
  for (const lose of [
    (h: ReturnType<typeof harness>) => h.peers[0].lose(),
    (h: ReturnType<typeof harness>) => h.peers[0].channel.onclose?.(),
    (h: ReturnType<typeof harness>) => h.streams[0].tracks[0].end(),
    (h: ReturnType<typeof harness>) => h.peers[0].remoteTrack.end(),
  ]) {
    const h = harness();
    await h.controller.start(target);
    lose(h);
    assert.equal(h.streams[0].tracks[0].enabled, false);
    await h.clock.advance(VOICE_RECOVERY_TIMEOUT_MS);
    assert.equal(h.controller.getSnapshot().phase, "error");
    assert.equal(h.streams[0].tracks[0].stopped, true);
    assert.equal(h.contexts[0].state, "closed");
    assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
    assert.equal(h.clock.timers.size, 0);
  }
});

test("mismatched active authority fails closed without ungating media", async () => {
  const h = harness();
  h.setRpc((method) => method === "mobkit/live/status" ? Promise.resolve({
    phase: "active", handle: {
      channel_id: "wrong-channel", target_identity: other.identity,
      execution_mode: "client_context", activation_receipt: "wrong-receipt",
    },
  }) : undefined);
  await h.controller.start(target);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.match(h.controller.getSnapshot().error!, /authority did not match/);
  assert.equal(h.streams[0].tracks[0].enabled, false);
  assert.equal(h.contexts[0].gains[0].gain.value, 0);
  assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
});

test("pagehide uses keepalive teardown, dispose is idempotent, and no subsequent start is admitted", async () => {
  const h = harness();
  await h.controller.start(target);
  h.pagehide();
  h.controller.dispose();
  await flush();
  assert.equal(h.pagehideCalls.length, 1);
  assert.equal(h.pagehideCalls[0].request_id, "request-1");
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.clock.timers.size, 0);
  await h.controller.start(other);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
});

test("server errors never surface raw credentials or transport data", async () => {
  const h = harness();
  h.setRpc((method) => method.endsWith("/open") ?
    Promise.reject(new Error("provider secret sk-real-secret token=credential SDP full-body")) : undefined);
  await h.controller.start(target);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.doesNotMatch(h.controller.getSnapshot().error!, /sk-real|credential|full-body/);
  assert.match(h.controller.getSnapshot().error!, /network and gateway/);
});

test("every post-open handshake failure fences the remote request and stops media", async () => {
  for (const stage of [
    "mobkit/live/playback_owner/register", "live/webrtc/answer",
    "mobkit/console/voice/answer_received", "mobkit/live/status",
  ]) {
    const h = harness();
    h.setRpc((method) => method === stage ? Promise.reject(new Error("handshake failure")) : undefined);
    await h.controller.start(target);
    assert.equal(h.controller.getSnapshot().phase, "error", stage);
    assert.equal(h.streams[0].tracks[0].stopped, true, stage);
    assert.equal(h.peers[0].connectionState, "closed", stage);
    assert.equal(h.contexts[0].state, "closed", stage);
    assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close", stage);
  }
});

test("an unconnected peer cannot activate merely because the server returns active authority", async () => {
  const h = harness();
  h.setPeer((peer) => {
    peer.setRemoteDescription = () => Promise.resolve();
  });
  const start = h.controller.start(target);
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.streams[0].tracks[0].enabled, false);
  await h.clock.advance(VOICE_CONNECT_TIMEOUT_MS);
  await start;
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.equal(h.streams[0].tracks[0].stopped, true);
});

test("remote output arriving before typed activation remains muted and does not count activity", async () => {
  const h = harness();
  const status = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/live/status" ? status.promise : undefined);
  const start = h.controller.start(target);
  await flush();
  assert.equal(h.contexts[0].gains[0].gain.value, 0);
  h.controller.toggleSpeaker();
  h.controller.toggleSpeaker();
  h.peers[0].channel.emit({ type: "response.output_text.delta", delta: "Not yet active" });
  assert.equal(h.contexts[0].gains[0].gain.value, 0);
  const samples = new Float32Array(16);
  h.contexts[0].analysers[1].signal = 0.2;
  h.controller.sampleWaveform("speaker", samples);
  assert.equal(samples[0], 0);
  await h.controller.close();
  await start;
  status.reject(new Error("late rejection after close"));
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "idle");
});

test("failed replacement teardown blocks further opens until explicit retry closes the original request", async () => {
  const h = harness();
  await h.controller.start(target);
  h.setRpc((method) => method.endsWith("/close") ? Promise.reject(new Error("offline")) : undefined);
  await h.controller.start(other);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.ok(h.streams.every((stream) => stream.tracks[0].stopped));
  await h.controller.start({ identity: "agent-c", label: "Agent C" });
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  h.setRpc(undefined);
  await h.controller.close();
  assert.equal(h.calls.at(-1)?.params.request_id, "request-1");
  assert.equal(h.controller.getSnapshot().phase, "idle");
  await h.controller.start(other);
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.controller.close();
});

test("browser audio interruption closes media-owner authority", async () => {
  const h = harness();
  await h.controller.start(target);
  h.contexts[0].state = "suspended";
  h.contexts[0].onstatechange?.();
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.match(h.controller.getSnapshot().error!, /audio was interrupted/);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
});

test("malformed data and transport keepalives are ignored; provider errors are sanitized and close voice", async () => {
  const h = harness();
  await h.controller.start(target);
  h.peers[0].channel.onmessage?.({ data: "not-json" });
  h.peers[0].channel.onmessage?.({ data: new ArrayBuffer(4) });
  h.peers[0].channel.emit(null);
  h.peers[0].channel.emit({ type: "rate_limits.updated" });
  assert.equal(h.controller.getSnapshot().phase, "active");
  h.peers[0].channel.emit({ type: "error", error: { message: "sk-provider-secret" } });
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.doesNotMatch(h.controller.getSnapshot().error!, /sk-provider-secret/);
});

test("synchronous observer close during requesting or activation cannot leak resources or enable audio", async () => {
  for (const phase of ["requesting", "active"]) {
    const h = harness();
    let close: Promise<void> | undefined;
    h.controller.subscribe(() => {
      if (h.controller.getSnapshot().phase === phase) close = h.controller.close();
    });
    await h.controller.start(target);
    await close;
    assert.equal(h.controller.getSnapshot().phase, "idle");
    assert.ok(h.streams.every((stream) => stream.tracks[0].stopped));
    assert.ok(h.contexts.every((context) => context.state === "closed"));
    assert.equal(h.clock.timers.size, 0);
  }
});

test("unavailable, unauthenticated, missing, and failed gateway availability never request mic or open voice", async () => {
  for (const available of [false, undefined, null, "true", 1]) {
    const h = harness();
    h.env.voiceAvailable = async () => available as boolean;
    await h.controller.start(target);
    assert.equal(h.controller.getSnapshot().phase, "error");
    assert.match(h.controller.getSnapshot().error!, /authenticate OpenAI/);
    assert.equal(h.streams.length, 0);
    assert.equal(h.peers.length, 0);
    assert.equal(h.calls.length, 0);
    assert.ok(h.contexts.every((context) => context.state === "closed"));
  }
  const h = harness();
  h.env.voiceAvailable = () => Promise.reject(new Error("offline"));
  await h.controller.start(target);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.equal(h.streams.length, 0);
  assert.equal(h.calls.length, 0);
});

test("availability is checked freshly on every start and stale checks cannot prompt for microphone", async () => {
  const h = harness();
  const availability = deferred<boolean>();
  h.env.voiceAvailable = () => availability.promise;
  const start = h.controller.start(target);
  await flush();
  assert.equal(h.streams.length, 0);
  await h.controller.close();
  availability.resolve(true);
  await start;
  await flush();
  assert.equal(h.streams.length, 0);
  assert.equal(h.calls.length, 0);
  h.env.voiceAvailable = () => Promise.resolve(true);
  await h.controller.start(target);
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.controller.close();
  const before = h.calls.length;
  h.env.voiceAvailable = () => Promise.resolve(false);
  await h.controller.start(target);
  assert.equal(h.streams.length, 1);
  assert.equal(h.calls.length, before);
});

test("StrictMode disposal can be followed by a fresh explicit start on the memoized controller", async () => {
  const h = harness();
  h.controller.dispose();
  let changes = 0;
  h.controller.subscribe(() => { changes++; });
  await h.controller.start(target);
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.ok(changes > 0);
  h.controller.dispose();
  const oldStream = h.streams[0];
  await h.controller.start(other);
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.equal(h.controller.getSnapshot().target?.identity, other.identity);
  assert.equal(oldStream.tracks[0].stopped, true);
  assert.equal(h.peers[0].connectionState, "closed");
  await h.controller.close();
  assert.equal(h.clock.timers.size, 0);
});

test("availability helper queries exact target readiness, accepts only explicit true, and fails closed", async () => {
  const originalFetch = globalThis.fetch;
  try {
    for (const body of [
      {}, { available: false }, { available: "true" }, { available: true },
      { identity: target.identity, available: false },
      { identity: target.identity, available: "true" },
      { identity: other.identity, available: true },
      { identity: target.identity.toUpperCase(), available: true },
      { identity: target.identity, available: true },
    ]) {
      globalThis.fetch = (async (input, init) => {
        assert.equal(input, "https://gateway.example/console/rpc");
        const request = JSON.parse(String(init?.body));
        assert.equal(request.method, "mobkit/console/voice/readiness");
        assert.deepEqual(request.params, { identity: target.identity });
        return new Response(JSON.stringify({ jsonrpc: "2.0", id: request.id, result: body }), { status: 200 });
      }) as typeof fetch;
      assert.equal(
        await queryVoiceAvailability("https://gateway.example", target.identity),
        body.identity === target.identity && body.available === true,
      );
    }
    globalThis.fetch = (async () => new Response("denied", { status: 403 })) as typeof fetch;
    assert.equal(await queryVoiceAvailability("", target.identity), false);
    globalThis.fetch = (async () => { throw new Error("offline"); }) as typeof fetch;
    assert.equal(await queryVoiceAvailability("", target.identity), false);
    globalThis.fetch = (async () => { assert.fail("Missing identity must not query readiness"); }) as typeof fetch;
    assert.equal(await queryVoiceAvailability("", ""), false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("readiness helper accepts the shared Rust HTTP golden contract for both availability states", async () => {
  const originalFetch = globalThis.fetch;
  let calls = 0;
  try {
    for (const [result, expected] of [
      [voiceContract.readiness_available, true],
      [voiceContract.readiness_unavailable, false],
    ] as const) {
      globalThis.fetch = (async (input, init) => {
        calls++;
        assert.equal(input, "https://gateway.example/console/rpc");
        const request = JSON.parse(String(init?.body));
        assert.equal(request.method, "mobkit/console/voice/readiness");
        assert.deepEqual(request.params, voiceContract.readiness_request);
        return new Response(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
      }) as typeof fetch;
      assert.equal(
        await queryVoiceAvailability("https://gateway.example", voiceContract.readiness_request.identity),
        expected,
      );
    }
    assert.equal(calls, 2);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("production browser environment constructs an empty MediaStream with a valid overload", async () => {
  const h = harness();
  const stream = new Stream();
  const controller = createVoiceSession("");
  const originalFetch = globalThis.fetch;
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const pause = vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  class BrowserStream extends Stream {
    constructor(tracks?: Track[]) {
      assert.ok(arguments.length === 0 || Array.isArray(tracks), "MediaStream(undefined) is not a valid browser overload");
      super(tracks ?? []);
    }
  }
  vi.stubGlobal("AudioContext", Context);
  vi.stubGlobal("RTCPeerConnection", Peer);
  vi.stubGlobal("MediaStream", BrowserStream);
  vi.stubGlobal("navigator", { mediaDevices: { getUserMedia: () => Promise.resolve(stream) } });
  vi.stubGlobal("crypto", { randomUUID: () => "browser-request" });
  globalThis.fetch = (async (input, init) => {
    const request = JSON.parse(String(init?.body));
    const result = await h.env.rpc(request.method, request.params, 30_000);
    return new Response(JSON.stringify({ jsonrpc: "2.0", id: request.id, result }));
  }) as typeof fetch;
  try {
    await controller.start(target);
    assert.equal(controller.getSnapshot().phase, "active", controller.getSnapshot().error ?? "");
    await controller.close();
    assert.equal(stream.tracks[0].stopped, true);
  } finally {
    await controller.close();
    controller.dispose();
    globalThis.fetch = originalFetch;
    play.mockRestore();
    pause.mockRestore();
    vi.unstubAllGlobals();
  }
});

test("fresh readiness receives the exact selected voice identity before any microphone capture", async () => {
  const h = harness();
  const checked: string[] = [];
  h.env.voiceAvailable = async (identity) => {
    checked.push(identity);
    assert.equal(h.streams.length, 0);
    return false;
  };
  await h.controller.start(other);
  assert.deepEqual(checked, [other.identity]);
  assert.equal(h.calls.length, 0);
});

test("owner-issued replacement reuses request and mic, preserves target/mutes and original silence deadline", async () => {
  for (const reason of ["canonical_context", "delegation_result"]) {
    const h = harness();
    await h.controller.start(target);
    h.controller.toggleMicrophone();
    h.controller.toggleSpeaker();
    const oldPeer = h.peers[0];
    const oldNodes = [...h.contexts[0].sources, ...h.contexts[0].analysers, ...h.contexts[0].gains];
    let required = true;
    h.setRpc((method) => {
      if (method !== "mobkit/console/voice/replacement") return undefined;
      const result = required ? replacement(reason) : { required: false };
      required = false;
      return Promise.resolve(result);
    });
    await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
    assert.equal(h.controller.getSnapshot().phase, "active");
    assert.deepEqual(h.controller.getSnapshot().target, target);
    assert.equal(h.controller.getSnapshot().microphoneMuted, true);
    assert.equal(h.controller.getSnapshot().speakerMuted, true);
    assert.equal(h.streams.length, 1, "recovery must not request microphone permission again");
    assert.equal(h.contexts.length, 1);
    assert.equal(h.streams[0].tracks[0].stopped, false, "existing microphone remains reusable");
    assert.equal(h.streams[0].tracks[0].enabled, false);
    assert.equal(oldPeer.remoteTrack.stopped, true);
    assert.equal(oldPeer.connectionState, "closed");
    assert.ok(oldNodes.every((node) => node.disconnected));
    assert.equal(h.peers.length, 2);
    assert.equal(h.contexts[0].gains[1].gain.value, 0);
    assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
    const register = h.calls.filter((call) => call.method === "mobkit/live/playback_owner/register").at(-1)!;
    assert.equal(register.params.channel_id, "recovery-channel");
    const poll = h.calls.find((call) => call.method === "mobkit/console/voice/replacement")!;
    assert.deepEqual(poll.params, { identity: target.identity, request_id: "request-1" });
    await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - VOICE_REPLACEMENT_POLL_INTERVAL_MS - 1);
    assert.equal(h.controller.getSnapshot().phase, "active");
    await h.clock.advance(1);
    assert.equal(h.controller.getSnapshot().phase, "idle");
    assert.equal(h.streams[0].tracks[0].stopped, true);
    assert.deepEqual(h.calls.at(-1)?.params, { identity: target.identity, request_id: "request-1" });
    assert.equal(h.clock.timers.size, 0);
  }
});

test("recovery keeps media gated until new typed activation and can be cancelled without late resurrection", async () => {
  const h = harness();
  await h.controller.start(target);
  const status = deferred<unknown>();
  h.setRpc((method, params) => {
    if (method === "mobkit/console/voice/replacement") return Promise.resolve(replacement());
    if (method === "mobkit/live/status" && params.channel_id === "recovery-channel") return status.promise;
    return undefined;
  });
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.streams[0].tracks[0].enabled, false);
  assert.equal(h.contexts[0].gains[1].gain.value, 0);
  assert.equal(h.peers[0].remoteTrack.stopped, true);
  h.controller.toggleMicrophone();
  h.controller.toggleMicrophone();
  h.controller.toggleSpeaker();
  h.controller.toggleSpeaker();
  assert.equal(h.streams[0].tracks[0].enabled, false);
  assert.equal(h.contexts[0].gains[1].gain.value, 0);
  await h.controller.close();
  status.resolve({
    phase: "active", handle: {
      channel_id: "recovery-channel", target_identity: target.identity,
      execution_mode: "client_context", activation_receipt: "late-receipt",
    },
  });
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.ok(h.peers.every((peer) => peer.connectionState === "closed"));
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  assert.equal(h.clock.timers.size, 0);
});

test("late old-peer events and playback promises cannot kill or ungate the replacement", async () => {
  const h = harness();
  const playback = deferred<void>();
  let firstAudio = true;
  h.env.createAudioElement = () => {
    const first = firstAudio;
    firstAudio = false;
    return {
      play: () => first ? playback.promise : Promise.resolve(), pause() {},
      autoplay: false, muted: false, srcObject: null,
    } as unknown as HTMLAudioElement;
  };
  await h.controller.start(target);
  const old = h.peers[0];
  const lateTrack = old.ontrack!;
  const lateConnection = old.onconnectionstatechange!;
  const lateMessage = old.channel.onmessage!;
  const lateClose = old.channel.onclose!;
  const lateError = old.channel.onerror!;
  const lateRemoteEnd = old.remoteTrack.onended!;
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? Promise.resolve(replacement()) : undefined);
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  const snapshot = h.controller.getSnapshot();
  const late = new Track();
  lateTrack({ track: late });
  lateConnection();
  lateMessage({ data: JSON.stringify({ type: "error" }) });
  lateClose();
  lateError();
  lateRemoteEnd();
  playback.reject(new Error("old audio player rejected after replacement"));
  await flush();
  assert.equal(late.stopped, true);
  assert.equal(h.controller.getSnapshot(), snapshot);
  assert.equal(snapshot.phase, "active");
  assert.equal(h.streams[0].tracks[0].enabled, true);
  await h.controller.close();
});

test("retained replacement response already bound does not create repeated peers", async () => {
  const h = harness();
  await h.controller.start(target);
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? Promise.resolve(replacement()) : undefined);
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS * 3);
  assert.equal(h.peers.length, 2);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.controller.close();
});

test("malformed, legacy, conflicting or cross-target replacement authority fails closed", async () => {
  const legacy = pending();
  delete (legacy as Partial<typeof legacy>).pending_receipt;
  for (const raw of [
    { required: "false" },
    replacement("unknown"),
    { ...replacement(), canonical_seed_cursor: -1 },
    replacement("canonical_context", legacy),
    replacement("canonical_context", pending(other.identity)),
    replacement("canonical_context", { ...pending(target.identity, `channel-${target.identity}`), pending_receipt: "conflict" }),
  ]) {
    const h = harness();
    await h.controller.start(target);
    h.setRpc((method) => method === "mobkit/console/voice/replacement" ? Promise.resolve(raw) : undefined);
    await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
    assert.equal(h.controller.getSnapshot().phase, "error");
    assert.equal(h.streams[0].tracks[0].stopped, true);
    assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
    assert.equal(h.clock.timers.size, 0);
  }
});

test("closed, denied or lost replacement polling fails closed and never impersonates no replacement", async () => {
  for (const kind of ["voice_closed", "access_denied", "transport_lost"]) {
    const h = harness();
    await h.controller.start(target);
    const error = Object.assign(new Error("internal detail"), { rpcError: { data: { kind } } });
    h.setRpc((method) => method === "mobkit/console/voice/replacement" ? Promise.reject(error) : undefined);
    await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
    assert.equal(h.controller.getSnapshot().phase, "error");
    if (kind === "voice_closed") assert.match(h.controller.getSnapshot().error!, /gateway closed/);
    assert.equal(h.streams[0].tracks[0].stopped, true);
    assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
  }
  const h = harness();
  await h.controller.start(target);
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? new Promise(() => {}) : undefined);
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS + VOICE_TEARDOWN_TIMEOUT_MS);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.equal(h.clock.timers.size, 0);
});

test("pending replacement polling is cancelled on close and its late response cannot start a new peer", async () => {
  const h = harness();
  await h.controller.start(target);
  const result = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? result.promise : undefined);
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  await h.controller.close();
  result.resolve(replacement());
  await flush();
  assert.equal(h.peers.length, 1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.equal(h.clock.timers.size, 0);
});

test("silence deadline can expire during recovery without waiting for connect timeout", async () => {
  const h = harness();
  await h.controller.start(target);
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - 2000);
  h.setRpc((method, params) => {
    if (method === "mobkit/console/voice/replacement") return Promise.resolve(replacement());
    if (method === "mobkit/live/status" && params.channel_id === "recovery-channel") return new Promise(() => {});
    return undefined;
  });
  await h.clock.advance(1000);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  await h.clock.advance(999);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.match(h.controller.getSnapshot().notice!, /15 minutes of silence/);
  assert.equal(h.clock.timers.size, 0);
});

test("new target start during recovery tears down the entire old request before opening the new target", async () => {
  const h = harness();
  await h.controller.start(target);
  const answer = deferred<unknown>();
  h.setRpc((method, params) => {
    if (method === "mobkit/console/voice/replacement") return Promise.resolve(replacement());
    if (method === "live/webrtc/answer" && params.channel_id === "recovery-channel") return answer.promise;
    return undefined;
  });
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  await h.controller.start(other);
  answer.resolve({ answer_sdp: "late-answer" });
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.equal(h.controller.getSnapshot().target?.identity, other.identity);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.peers[0].connectionState, "closed");
  assert.equal(h.peers[1].connectionState, "closed");
  const newOpen = h.calls.findIndex((call) => call.method.endsWith("/open") && call.params.identity === other.identity);
  const oldClose = h.calls.findIndex((call) => call.method.endsWith("/close"));
  assert.ok(oldClose < newOpen);
  await h.controller.close();
});

test("old peer closing before the next poll discovers owner recovery instead of cancelling its request", async () => {
  for (const lose of [
    (peer: Peer) => { peer.connectionState = "closed"; peer.onconnectionstatechange?.(); },
    (peer: Peer) => { peer.channel.onclose?.(); },
    (peer: Peer) => { peer.remoteTrack.end(); },
  ]) {
    const h = harness();
    await h.controller.start(target);
    const oldPeer = h.peers[0];
    const lateState = oldPeer.onconnectionstatechange!;
    h.setRpc((method) => method === "mobkit/console/voice/replacement" ? Promise.resolve(replacement()) : undefined);
    lose(oldPeer);
    assert.equal(h.controller.getSnapshot().phase, "connecting");
    assert.equal(h.streams[0].tracks[0].enabled, false);
    assert.equal(h.streams[0].tracks[0].stopped, false);
    await flush();
    assert.equal(h.controller.getSnapshot().phase, "active");
    assert.deepEqual(h.controller.getSnapshot().target, target);
    assert.equal(h.peers.length, 2);
    assert.equal(h.streams.length, 1);
    assert.equal(h.streams[0].tracks[0].enabled, true);
    lateState();
    assert.equal(h.controller.getSnapshot().phase, "active");
    assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
    assert.equal(h.calls.filter((call) => call.method.endsWith("/close")).length, 0);
    await h.controller.close();
  }
});

test("transport loss joins an in-flight request-owned replacement check instead of racing a second poll", async () => {
  const h = harness();
  await h.controller.start(target);
  const result = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? result.promise : undefined);
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  const lateConnection = h.peers[0].onconnectionstatechange!;
  h.peers[0].lose();
  lateConnection();
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.calls.filter((call) => call.method.endsWith("/replacement")).length, 1);
  result.resolve({ required: false });
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? Promise.resolve(replacement()) : undefined);
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.equal(h.peers.length, 2);
  await h.controller.close();
});

test("lost transport fails closed on consumed authority or a stalled RPC without opening fresh", async () => {
  for (const response of [
    () => Promise.resolve(replacement("canonical_context", pending(target.identity, `channel-${target.identity}`))),
    () => new Promise<unknown>(() => {}),
  ]) {
    const h = harness();
    await h.controller.start(target);
    h.setRpc((method) => method === "mobkit/console/voice/replacement" ? response() : undefined);
    h.peers[0].lose();
    assert.equal(h.streams[0].tracks[0].enabled, false);
    await h.clock.advance(VOICE_TEARDOWN_TIMEOUT_MS);
    assert.equal(h.controller.getSnapshot().phase, "error");
    assert.equal(h.streams[0].tracks[0].stopped, true);
    assert.equal(h.calls.filter((call) => call.method.endsWith("/replacement")).length, 1);
    assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
    assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
    assert.equal(h.clock.timers.size, 0);
  }
});

test("transport loss waits for delayed owner replacement while preserving target, mutes and silence", async () => {
  const h = harness();
  await h.controller.start(target);
  h.controller.toggleMicrophone();
  h.controller.toggleSpeaker();
  await h.clock.advance(10_000);
  let polls = 0;
  h.setRpc((method) => {
    if (method !== "mobkit/console/voice/replacement") return undefined;
    polls++;
    return Promise.resolve(polls <= 3 ? { required: false } : replacement());
  });
  h.peers[0].lose();
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.streams[0].tracks[0].enabled, false);
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS * 3 - 1);
  assert.equal(polls, 3);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.calls.filter((call) => call.method.endsWith("/close")).length, 0);
  await h.clock.advance(1);
  assert.equal(polls, 4);
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.deepEqual(h.controller.getSnapshot().target, target);
  assert.equal(h.controller.getSnapshot().microphoneMuted, true);
  assert.equal(h.controller.getSnapshot().speakerMuted, true);
  assert.equal(h.contexts[0].gains[1].gain.value, 0);
  assert.equal(h.streams.length, 1);
  assert.equal(h.peers.length, 2);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  h.setRpc(undefined);
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - h.clock.now - 1);
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.match(h.controller.getSnapshot().notice!, /15 minutes of silence/);
  assert.equal(h.clock.timers.size, 0);
});

test("false replacement replies and repeated old-peer loss cannot extend the fixed recovery deadline", async () => {
  const h = harness();
  await h.controller.start(target);
  const old = h.peers[0];
  const lateState = old.onconnectionstatechange!;
  const lateClose = old.channel.onclose!;
  let polls = 0;
  h.setRpc((method) => {
    if (method !== "mobkit/console/voice/replacement") return undefined;
    polls++;
    lateState();
    lateClose();
    return Promise.resolve({ required: false });
  });
  old.lose();
  await h.clock.advance(VOICE_RECOVERY_TIMEOUT_MS - 1);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.streams[0].tracks[0].enabled, false);
  assert.equal(h.streams[0].tracks[0].stopped, false);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/close")).length, 0);
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.match(h.controller.getSnapshot().error!, /recovery timed out/);
  assert.equal(polls, VOICE_RECOVERY_TIMEOUT_MS / VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/close")).length, 1);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.clock.timers.size, 0);
});

test("late pending discovery cannot extend the recovery deadline with a new activation timeout", async () => {
  const h = harness();
  await h.controller.start(target);
  h.setRpc((method, params) => {
    if (method === "mobkit/console/voice/replacement") {
      return Promise.resolve(h.clock.now < VOICE_RECOVERY_TIMEOUT_MS - 1000 ? { required: false } : replacement());
    }
    if (method === "mobkit/live/status" && params.channel_id === "recovery-channel") return new Promise(() => {});
    return undefined;
  });
  h.peers[0].lose();
  await h.clock.advance(VOICE_RECOVERY_TIMEOUT_MS - 1);
  assert.equal(h.peers.length, 2);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.match(h.controller.getSnapshot().error!, /recovery timed out/);
  assert.ok(h.peers.every((peer) => peer.connectionState === "closed"));
  assert.equal(h.clock.timers.size, 0);
});

test("explicit cancel during delayed replacement discovery fences late pending without a new peer", async () => {
  const h = harness();
  await h.controller.start(target);
  const late = deferred<unknown>();
  let polls = 0;
  h.setRpc((method) => {
    if (method !== "mobkit/console/voice/replacement") return undefined;
    polls++;
    return polls <= 2 ? Promise.resolve({ required: false }) : late.promise;
  });
  h.peers[0].lose();
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS * 2);
  assert.equal(polls, 3);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  await h.controller.close();
  late.resolve(replacement());
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.equal(h.peers.length, 1);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.clock.timers.size, 0);
});

test("original audio silence deadline expires during false recovery replies before the recovery deadline", async () => {
  const h = harness();
  await h.controller.start(target);
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS - 2000);
  h.peers[0].lose();
  await h.clock.advance(1999);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.streams[0].tracks[0].enabled, false);
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.match(h.controller.getSnapshot().notice!, /15 minutes of silence/);
  assert.equal(h.peers.length, 1);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.clock.timers.size, 0);
});

test("terminal authorization errors stop recovery waiting immediately", async () => {
  const h = harness();
  await h.controller.start(target);
  let polls = 0;
  h.setRpc((method) => {
    if (method !== "mobkit/console/voice/replacement") return undefined;
    polls++;
    return polls < 3
      ? Promise.resolve({ required: false })
      : Promise.reject(Object.assign(new Error("access revoked"), {
        rpcError: { code: -32030, data: { kind: "access_denied" } },
      }));
  });
  h.peers[0].lose();
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS * 2);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/open")).length, 1);
  assert.equal(h.clock.timers.size, 0);
});

test("explicit close wins over a transport-loss recovery check and fences its late pending handle", async () => {
  const h = harness();
  await h.controller.start(target);
  const result = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? result.promise : undefined);
  h.peers[0].lose();
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  await h.controller.close();
  result.resolve(replacement());
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.equal(h.peers.length, 1);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.clock.timers.size, 0);
});

test("answer delivery ACK follows remote description and precedes status or media release", async () => {
  const h = harness();
  const receipt = deferred<unknown>();
  h.setRpc((method, params) => {
    if (method !== "mobkit/console/voice/answer_received") return undefined;
    assert.equal(h.peers[0].connectionState, "connected", "answer must have been applied locally");
    assert.equal(h.streams[0].tracks[0].enabled, false);
    assert.equal(h.contexts[0].gains[0].gain.value, 0);
    assert.deepEqual(params, {
      identity: target.identity, request_id: "request-1", channel_id: `channel-${target.identity}`,
    });
    return receipt.promise;
  });
  const start = h.controller.start(target);
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.calls.some((call) => call.method === "mobkit/live/status"), false);
  receipt.resolve({ accepted: true });
  await start;
  assert.equal(h.controller.getSnapshot().phase, "active");
  await h.controller.close();
});

test("missing or denied answer delivery ACK fails closed without querying activation", async () => {
  for (const result of [null, {}, [], { accepted: false }, { accepted: "true" }]) {
    const h = harness();
    h.setRpc((method) => method === "mobkit/console/voice/answer_received" ? Promise.resolve(result) : undefined);
    await h.controller.start(target);
    assert.equal(h.controller.getSnapshot().phase, "error");
    assert.match(h.controller.getSnapshot().error!, /acknowledge voice answer delivery/);
    assert.equal(h.calls.some((call) => call.method === "mobkit/live/status"), false);
    assert.equal(h.streams[0].tracks[0].stopped, true);
    assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
  }
});

test("each recovery answer has its own channel-scoped delivery ACK, cancelled late ACK cannot activate", async () => {
  const h = harness();
  await h.controller.start(target);
  const receipt = deferred<unknown>();
  h.setRpc((method, params) => {
    if (method === "mobkit/console/voice/replacement") return Promise.resolve(replacement());
    if (method === "mobkit/console/voice/answer_received" && params.channel_id === "recovery-channel") {
      assert.deepEqual(params, {
        identity: target.identity, request_id: "request-1", channel_id: "recovery-channel",
      });
      return receipt.promise;
    }
    return undefined;
  });
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS);
  assert.equal(h.controller.getSnapshot().phase, "connecting");
  assert.equal(h.calls.filter((call) => call.method === "mobkit/console/voice/answer_received").length, 2);
  assert.equal(h.calls.some((call) => call.method === "mobkit/live/status" && call.params.channel_id === "recovery-channel"), false);
  await h.controller.close();
  receipt.resolve({ accepted: true });
  await flush();
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.clock.timers.size, 0);
});

test("actual audio reports activity immediately, throttles to five seconds, and silence never sends heartbeats", async () => {
  const h = harness();
  const times: number[] = [];
  h.setRpc((method, params) => {
    if (method !== "mobkit/console/voice/activity") return undefined;
    times.push(h.clock.now);
    assert.deepEqual(params, { identity: target.identity, request_id: "request-1" });
    return Promise.resolve({ accepted: true });
  });
  await h.controller.start(target);
  assert.deepEqual(times, []);
  h.contexts[0].analysers[0].signal = 0.25;
  await h.clock.advance(100);
  assert.deepEqual(times, [100], "first observed audio reports without debounce delay");
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS - 1);
  assert.deepEqual(times, [100]);
  h.contexts[0].analysers[0].signal = 0;
  await h.clock.advance(1);
  assert.deepEqual(times, [100, 5100]);
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS * 3);
  assert.deepEqual(times, [100, 5100], "elapsed timers alone must never report activity");
  await h.controller.close();
});

test("muted mic, text and quiet frames never report activity; incoming model audio counts with muted speaker", async () => {
  const h = harness();
  await h.controller.start(target);
  h.controller.toggleMicrophone();
  h.controller.toggleSpeaker();
  h.contexts[0].analysers[0].signal = 1;
  h.peers[0].channel.emit({ type: "input_audio_buffer.speech_started" });
  h.peers[0].channel.emit({ type: "response.output_text.delta", delta: "Only text" });
  h.peers[0].channel.emit({ type: "response.output_audio_transcript.delta", delta: "Only transcript" });
  h.peers[0].channel.emit({ type: "response.output_audio.delta", delta: "AAAA" });
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS * 2);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/activity")).length, 0);
  h.contexts[0].analysers[1].signal = 0.5;
  await h.clock.advance(100);
  assert.equal(h.contexts[0].gains[0].gain.value, 0);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/activity")).length, 1);
  await h.controller.close();
});

test("activity reports never overlap; stalled confirmation fails closed instead of pretending alive", async () => {
  const h = harness();
  h.setRpc((method) => method === "mobkit/console/voice/activity" ? new Promise(() => {}) : undefined);
  await h.controller.start(target);
  h.contexts[0].analysers[0].signal = 0.5;
  await h.clock.advance(100);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/activity")).length, 1);
  await h.clock.advance(VOICE_TEARDOWN_TIMEOUT_MS - 1);
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.equal(h.calls.filter((call) => call.method.endsWith("/activity")).length, 1);
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "error");
  assert.match(h.controller.getSnapshot().error!, /activity could not be confirmed/);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/activity")).length, 1);
  assert.equal(h.streams[0].tracks[0].stopped, true);
  assert.equal(h.clock.timers.size, 0);
});

test("failed or invalid activity acceptance closes voice and clears its silence deadline", async () => {
  for (const response of [
    () => Promise.resolve({ accepted: false }),
    () => Promise.resolve({ accepted: "true" }),
    () => Promise.reject(new Error("network failed")),
  ]) {
    const h = harness();
    h.setRpc((method) => method === "mobkit/console/voice/activity" ? response() : undefined);
    await h.controller.start(target);
    h.contexts[0].analysers[1].signal = 0.5;
    await h.clock.advance(100);
    assert.equal(h.controller.getSnapshot().phase, "error");
    assert.equal(h.streams[0].tracks[0].stopped, true);
    assert.equal(h.calls.at(-1)?.method, "mobkit/console/voice/close");
    assert.equal(h.clock.timers.size, 0);
  }
});

test("late activity acknowledgement after close is inert", async () => {
  const h = harness();
  const accepted = deferred<unknown>();
  h.setRpc((method) => method === "mobkit/console/voice/activity" ? accepted.promise : undefined);
  await h.controller.start(target);
  h.contexts[0].analysers[0].signal = 0.5;
  await h.clock.advance(100);
  await h.controller.close();
  const snapshot = h.controller.getSnapshot();
  accepted.resolve({ accepted: true });
  await flush();
  assert.equal(h.controller.getSnapshot(), snapshot);
  assert.equal(snapshot.phase, "idle");
  assert.equal(h.clock.timers.size, 0);
});

test("activity uses the same request scope across owner-issued recovery, never old channel authority", async () => {
  const h = harness();
  await h.controller.start(target);
  h.peers[0].channel.emit({ type: "input_audio_buffer.speech_started" });
  await flush();
  h.setRpc((method) => method === "mobkit/console/voice/replacement" ? Promise.resolve(replacement()) : undefined);
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS);
  assert.equal(h.peers.length, 2);
  h.peers[1].channel.emit({ type: "input_audio_buffer.speech_started" });
  await flush();
  const reports = h.calls.filter((call) => call.method === "mobkit/console/voice/activity");
  assert.equal(reports.length, 2);
  assert.ok(reports.every((call) => Object.keys(call.params).length === 2));
  assert.deepEqual(reports.map((call) => call.params), [
    { identity: target.identity, request_id: "request-1" },
    { identity: target.identity, request_id: "request-1" },
  ]);
  await h.controller.close();
});

test("unreported real audio during cooldown flushes once after quiet without changing the local silence deadline", async () => {
  const h = harness();
  const reports: number[] = [];
  h.setRpc((method) => {
    if (method !== "mobkit/console/voice/activity") return undefined;
    reports.push(h.clock.now);
    return Promise.resolve({ accepted: true });
  });
  await h.controller.start(target);
  h.contexts[0].analysers[0].signal = 0.5;
  await h.clock.advance(200);
  h.contexts[0].analysers[0].signal = 0;
  assert.deepEqual(reports, [100]);
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS - 100);
  assert.deepEqual(reports, [100, 5100], "the last real sample at 200ms must reach the server");
  await h.clock.advance(VOICE_SILENCE_TIMEOUT_MS + 200 - h.clock.now - 1);
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.deepEqual(reports, [100, 5100], "quiet must not perpetually re-arm trailing reports");
  await h.clock.advance(1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.match(h.controller.getSnapshot().notice!, /15 minutes of silence/);
  assert.equal(h.clock.timers.size, 0);
});

test("audio received during an in-flight report remains dirty for one trailing flush, without overlap", async () => {
  const h = harness();
  const accepted = deferred<unknown>();
  let reports = 0;
  h.setRpc((method) => {
    if (method !== "mobkit/console/voice/activity") return undefined;
    reports++;
    return reports === 1 ? accepted.promise : Promise.resolve({ accepted: true });
  });
  await h.controller.start(target);
  h.contexts[0].analysers[0].signal = 0.5;
  await h.clock.advance(300);
  h.contexts[0].analysers[0].signal = 0;
  assert.equal(reports, 1);
  accepted.resolve({ accepted: true });
  await flush();
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS - 200);
  assert.equal(reports, 2);
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS * 2);
  assert.equal(reports, 2);
  await h.controller.close();
});

test("close cancels a dirty trailing activity flush", async () => {
  const h = harness();
  await h.controller.start(target);
  h.contexts[0].analysers[0].signal = 0.5;
  await h.clock.advance(200);
  h.contexts[0].analysers[0].signal = 0;
  await h.controller.close();
  await h.clock.advance(VOICE_ACTIVITY_REPORT_INTERVAL_MS * 2);
  assert.equal(h.calls.filter((call) => call.method.endsWith("/activity")).length, 1);
  assert.equal(h.controller.getSnapshot().phase, "idle");
  assert.equal(h.clock.timers.size, 0);
});

test("provider-managed unmeasured voice needs no output observer or browser playback settlement", async () => {
  const h = harness();
  await h.controller.start(target);
  h.peers[0].channel.emit({ type: "response.done" });
  h.peers[0].channel.emit({ type: "output_audio_buffer.stopped" });
  h.peers[0].channel.emit({
    type: "live.assistant_output",
    output: { channel_id: `channel-${target.identity}`, output_id: "untrusted", content_index: 0 },
  });
  await h.clock.advance(VOICE_REPLACEMENT_POLL_INTERVAL_MS * 2);
  assert.equal(h.controller.getSnapshot().phase, "active");
  assert.equal(h.calls.some((call) => /outputs\/|playback_complete|truncate/.test(call.method)), false);
  assert.equal(h.calls.some((call) => call.method.endsWith("/activity")), false);
  await h.controller.close();
});
