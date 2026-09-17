#!/usr/bin/env node

const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const http = require("node:http");
const path = require("node:path");
const { chromium } = require("playwright");

const agents = ["Alpha", "Beta"].map((label) => ({
  identity: `identity:${label.toLowerCase()}`,
  member_id: `identity:${label.toLowerCase()}`,
  agent_id: `identity:${label.toLowerCase()}`,
  label,
  kind: "identity",
  state: "active",
  addressable: true,
  affordances: { addressable: true, can_send_message: true },
}));

function pendingHandle(channelId, identity) {
  return {
    channel_id: channelId,
    target_identity: identity,
    execution_mode: "client_context",
    pending_receipt: `pending-${channelId}`,
    transport: { transport: "webrtc", token: `token-${channelId}`, answer_method: "live/webrtc/answer" },
    capabilities: {
      audio_in: true, audio_out: true, text_in: false, text_out: false,
      image_in: false, video_in: false, transcript_supported: true,
      barge_in_supported: true, provider_native_resume: false,
    },
    continuity: { mode: "transcript_only" },
  };
}

async function startServer() {
  const files = new Map(await Promise.all([
    ["/console", "index.html", "text/html"],
    ["/console/assets/console-app.js", "console-app.js", "application/javascript"],
    ["/console/assets/console-app.css", "console-app.css", "text/css"],
  ].map(async ([url, file, type]) => [url, { bytes: await fs.readFile(path.join(__dirname, "dist", file)), type }])));
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://localhost");
    const file = files.get(url.pathname);
    if (file) {
      res.writeHead(200, { "content-type": file.type });
      res.end(file.bytes);
    } else {
      res.writeHead(404);
      res.end("Not found");
    }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  return { server, url: `http://127.0.0.1:${server.address().port}` };
}

async function installConsoleFixture(page, { available = true } = {}) {
  const requests = [];
  let sequence = 0;
  const channels = new Map();
  const replacements = new Map();
  const preparing = new Set();
  const cancelled = new Set();
  await page.route("**/console/experience", (route) => route.fulfill({ json: {
    contract_version: "0.5.0",
    runtime_id: "voice-browser-fixture",
    voice: { available: false, readiness_method: "mobkit/console/voice/readiness" },
    runtime_capabilities: { can_send_messages: true },
    console_config: {
      title: "Voice fixture",
      appearance: { default_theme: "dark", default_variant: "graphite" },
      layout: { initial_agent: "identity:alpha", initial_preset: "single", sidebar_collapsed: false },
      rail: { visible: false },
    },
    agent_sidebar: { live_snapshot: { agents } },
    identity_status: { rows: agents.map((agent) => ({ ...agent, display_name: agent.label, addressability: "addressable" })) },
  } }));
  await page.route("**/console/modules", (route) => route.fulfill({ json: { modules: [] } }));
  await page.route("**/console/identities", (route) => route.fulfill({ json: { rows: agents } }));
  await page.route("**/console/timeline?*", (route) => route.fulfill({ json: { frames: [], available: true } }));
  await page.route("**/console/timeline/stream*", (route) => route.fulfill({
    contentType: "text/event-stream",
    body: ": fixture\n\n",
  }));
  await page.route("**/console/rpc", async (route) => {
    const { id, method, params = {} } = route.request().postDataJSON();
    requests.push({ method, params });
    const respond = (result) => route.fulfill({ json: { jsonrpc: "2.0", id, result } });
    if (method === "mobkit/capabilities") {
      return respond({ methods: ["mobkit/console/send"], feature_capabilities: available ? ["live.execution_identity.v1", "live.execution.client_context.v1"] : [] });
    }
    if (method === "mobkit/console/voice/readiness") {
      return respond({ identity: params.identity, available: available && agents.some((agent) => agent.identity === params.identity) });
    }
    if (method === "mobkit/console/voice/replacement") {
      const channel = [...channels.values()].find((channel) => channel.requestId === params.request_id && channel.identity === params.identity && !channel.closed);
      if (cancelled.has(params.request_id) || (!channel && !preparing.has(params.request_id))) {
        return route.fulfill({ json: { jsonrpc: "2.0", id, error: { code: -32000, message: "Voice closed", data: { kind: "voice_closed" } } } });
      }
      return respond(replacements.get(params.request_id) ?? { required: false });
    }
    if (method === "mobkit/console/voice/context_status") {
      const channel = channels.get(params.channel_id);
      assert.ok(channel && !channel.closed && channel.identity === params.identity &&
        channel.requestId === params.request_id && !cancelled.has(params.request_id),
      "context status must address the current owned channel");
      return respond({ ...params, context_preparation: channel.contextPreparation });
    }
    if (method === "mobkit/console/voice/answer_received" || method === "mobkit/console/voice/activity") {
      const channel = [...channels.entries()].find(([channelId, candidate]) =>
        candidate.identity === params.identity && candidate.requestId === params.request_id &&
        !cancelled.has(params.request_id) &&
        (method.endsWith("/activity") || (!candidate.closed && channelId === params.channel_id)));
      assert.ok(channel, `${method} must address the current request`);
      if (method.endsWith("/answer_received")) channel[1].answerAccepted = true;
      else assert.equal(channel[1].answerAccepted, true, "activity must follow activation");
      return respond({ accepted: true });
    }
    if (method === "mobkit/console/voice/open") {
      assert.equal(available, true, "voice must not open without OpenAI auth");
      assert.equal(typeof params.request_id, "string");
      const channelId = `voice-${++sequence}`;
      channels.set(channelId, {
        identity: params.identity, requestId: params.request_id, closed: false, answerAccepted: false,
        contextPreparation: { phase: "preparing", stage: "generating" },
      });
      return respond(pendingHandle(channelId, params.identity));
    }
    if (method === "mobkit/live/playback_owner/register") {
      return respond({ channel_id: params.channel_id, readiness_receipt: `ready-${params.channel_id}` });
    }
    if (method === "live/webrtc/answer") {
      const answerSdp = await page.evaluate(async ({ offer, channelId }) => {
        const peer = new RTCPeerConnection();
        const context = new AudioContext();
        await context.resume();
        const oscillator = context.createOscillator();
        oscillator.frequency.value = 180;
        const volume = context.createGain();
        volume.gain.value = 0.09;
        const output = context.createMediaStreamDestination();
        oscillator.connect(volume).connect(output);
        oscillator.start();
        for (const track of output.stream.getTracks()) peer.addTrack(track, output.stream);
        peer.addEventListener("datachannel", (event) => {
          window.voiceFixture.channels.push(event.channel);
        });
        window.voiceFixture.providers.set(channelId, { peer, context, oscillator });
        await peer.setRemoteDescription({ type: "offer", sdp: offer });
        await peer.setLocalDescription(await peer.createAnswer());
        if (peer.iceGatheringState !== "complete") {
          await new Promise((resolve, reject) => {
            const timeout = setTimeout(() => reject(new Error("Fixture ICE gathering timed out")), 5000);
            peer.addEventListener("icegatheringstatechange", () => {
              if (peer.iceGatheringState === "complete") {
                clearTimeout(timeout);
                resolve();
              }
            });
          });
        }
        return peer.localDescription.sdp;
      }, { offer: params.offer_sdp, channelId: params.channel_id });
      for (const [requestId, replacement] of replacements) {
        if (replacement.replacement.channel_id === params.channel_id) replacements.delete(requestId);
      }
      return respond({ answer_sdp: answerSdp });
    }
    if (method === "mobkit/live/status") {
      const channel = channels.get(params.channel_id);
      assert.ok(channel, "status must address an opened channel");
      if (!channel.closed && !channel.answerAccepted) return respond({ phase: "pending" });
      return respond(channel.closed ? { phase: "closed" } : {
        phase: "active",
        handle: {
          channel_id: params.channel_id, target_identity: channel.identity,
          execution_mode: "client_context", activation_receipt: `active-${params.channel_id}`,
        },
      });
    }
    if (method === "mobkit/console/voice/close") {
      const owned = [...channels.values()].filter((channel) => channel.requestId === params.request_id && channel.identity === params.identity);
      assert.ok(owned.length, "close must address an opened request");
      owned.forEach((channel) => { channel.closed = true; });
      replacements.delete(params.request_id);
      preparing.delete(params.request_id);
      cancelled.add(params.request_id);
      return respond({ phase: "closed" });
    }
    if (method === "mobkit/live/replacement_required") return respond({ required: false });
    if (method === "mobkit/live/playback_complete") return respond({ status: "completed" });
    if (method === "mobkit/console/send") {
      return respond({ interaction_id: `text-${++sequence}`, identity: params.identity, accepted: true });
    }
    if (method === "mobkit/console/query_timeline") return respond({ frames: [], available: true });
    return route.fulfill({ json: { jsonrpc: "2.0", id, error: { code: -32601, message: `Fixture does not implement ${method}` } } });
  });
  await page.addInitScript(() => {
    window.voiceFixture = { microphoneTracks: [], providers: new Map(), channels: [] };
    const getUserMedia = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
    navigator.mediaDevices.getUserMedia = async (constraints) => {
      const stream = await getUserMedia(constraints);
      window.voiceFixture.microphoneTracks.push(...stream.getTracks());
      return stream;
    };
  });
  return {
    requests,
    channels,
    setAvailable(value) { available = value; },
    setContext(identity, preparation) {
      const channel = [...channels.values()].find((candidate) => candidate.identity === identity && !candidate.closed);
      assert.ok(channel, "context transition requires an active fixture channel");
      channel.contextPreparation = preparation;
    },
    prepareReplacement(identity) {
      const current = [...channels.entries()].find(([, channel]) => channel.identity === identity && !channel.closed);
      assert.ok(current);
      const [previousId, previous] = current;
      previous.closed = true;
      preparing.add(previous.requestId);
      return {
        previousId,
        publish() {
          assert.equal(cancelled.has(previous.requestId), false, "recovery must not cancel a request while its replacement is preparing");
          const channelId = `voice-${++sequence}`;
          channels.set(channelId, {
            identity, requestId: previous.requestId, closed: false, answerAccepted: false,
            contextPreparation: { phase: "preparing", stage: "generating" },
          });
          replacements.set(previous.requestId, {
            required: true,
            reason: "canonical_context",
            replacement: pendingHandle(channelId, identity),
            canonical_seed_cursor: 12,
          });
          preparing.delete(previous.requestId);
        },
      };
    },
  };
}

async function openAgent(page, label) {
  await page.locator('.agent[role="button"]').filter({ hasText: label }).first().click();
  await page.getByTestId(`chat-composer:identity:${label.toLowerCase()}`).waitFor();
}

async function sendText(page, identity, text) {
  await page.getByTestId(`chat-composer:${identity}`).fill(text);
  const sent = page.waitForResponse((response) => response.url().endsWith("/console/rpc") &&
    response.request().postDataJSON()?.method === "mobkit/console/send");
  await page.getByTestId(`chat-send:${identity}`).click();
  await sent;
}

async function waitForVoice(page, requests) {
  try {
    await page.locator('[data-testid="voice-bar"][data-phase="active"]').waitFor({ timeout: 20000 });
  } catch (error) {
    throw new Error(`Voice did not activate: ${await page.getByTestId("voice-bar").textContent()}\nRPCs: ${requests.map((request) => request.method).join(", ")}`, { cause: error });
  }
}

async function assertVoiceLayout(page) {
  const layout = await page.getByTestId("voice-bar").evaluate((bar) => {
    const bounds = bar.getBoundingClientRect();
    const controls = bar.querySelector(".voice-bar__controls").getBoundingClientRect();
    return {
      width: bounds.width,
      right: bounds.right,
      viewport: window.innerWidth,
      overflowingControls: [...bar.querySelectorAll("button")].filter((button) => {
        const rect = button.getBoundingClientRect();
        return rect.left < bounds.left || rect.right > bounds.right;
      }).map((button) => button.getAttribute("aria-label")),
      overflowingText: [...bar.querySelectorAll(".voice-bar__status, .voice-bar__message")].filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.left < bounds.left || rect.right > bounds.right || element.scrollWidth > element.clientWidth + 1;
      }).map((element) => element.textContent),
      overlappingHeaderText: [...bar.querySelectorAll(".voice-bar__name, .voice-bar__status > span")].filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.right > controls.left && rect.left < controls.right &&
          rect.bottom > controls.top && rect.top < controls.bottom;
      }).map((element) => element.textContent),
    };
  });
  assert.ok(layout.width > 0 && layout.right <= layout.viewport, JSON.stringify(layout));
  assert.deepEqual(layout.overflowingControls, [], JSON.stringify(layout));
  assert.deepEqual(layout.overflowingText, [], JSON.stringify(layout));
  assert.deepEqual(layout.overlappingHeaderText, [], JSON.stringify(layout));
}

async function main() {
  const { server, url } = await startServer();
  let browser;
  try {
    browser = await chromium.launch({
      headless: true,
      args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"],
    });
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, permissions: ["microphone"] });
    const page = await context.newPage();
    if (process.env.VOICE_DEBUG) {
      const debuggerSession = await context.newCDPSession(page);
      await debuggerSession.send("Debugger.enable");
      await debuggerSession.send("Debugger.setPauseOnExceptions", { state: "all" });
      debuggerSession.on("Debugger.paused", async (event) => {
        console.error("Browser exception:", event.data?.description);
        await debuggerSession.send("Debugger.resume");
      });
    }
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    const fixture = await installConsoleFixture(page);
    await page.goto(`${url}/console`);
    await openAgent(page, "Alpha");
    await page.getByRole("button", { name: "Start voice with Alpha" }).click();
    await waitForVoice(page, fixture.requests);
    await page.getByRole("region", { name: "Voice with Alpha" }).waitFor();
    await page.getByText("Preparing context", { exact: true }).waitFor();
    assert.equal(await page.locator(".voice-waveform").count(), 2);
    await sendText(page, "identity:alpha", "Keep working while we talk");
    await page.locator('[data-testid="voice-bar"][data-phase="active"]').waitFor();
    assert.ok(fixture.requests.some((request) => request.method === "mobkit/console/send" && request.params.identity === "identity:alpha"));
    await page.getByRole("button", { name: "Mute microphone" }).click();
    assert.equal(await page.evaluate(() => window.voiceFixture.microphoneTracks.at(-1).enabled), false);
    await page.getByRole("button", { name: "Mute speakers" }).click();
    await page.getByRole("button", { name: "Unmute speakers" }).waitFor();
    await page.waitForFunction(() => {
      const canvas = document.querySelector(".voice-waveform--speaker");
      const context = canvas.getContext("2d");
      const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
      // A muted speaker must still display real incoming audio, not a flat idle line.
      for (let y = 0; y < canvas.height / 2 - 4; y += 1) {
        for (let x = 0; x < canvas.width; x += 1) {
          if (pixels[(y * canvas.width + x) * 4 + 3] > 100) return true;
        }
      }
      return false;
    });
    assert.equal(await page.getByText("Preparing context", { exact: true }).count(), 1,
      "native audio, text and mute controls must work before the context gate is released");
    fixture.setContext("identity:alpha", { phase: "provider_acknowledged" });
    await page.getByText("Context supplied", { exact: true }).waitFor();
    const capturesBeforeRecovery = await page.evaluate(() => window.voiceFixture.microphoneTracks.length);
    const replacement = fixture.prepareReplacement("identity:alpha");
    await page.evaluate(async (channelId) => {
      const provider = window.voiceFixture.providers.get(channelId);
      provider.peer.close();
      provider.oscillator.stop();
      await provider.context.close();
    }, replacement.previousId);
    await page.locator('[data-testid="voice-bar"][data-phase="connecting"]').waitFor();
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const preparingResponse = await page.waitForResponse((response) =>
        response.url().endsWith("/console/rpc") &&
        response.request().postDataJSON()?.method === "mobkit/console/voice/replacement");
      assert.deepEqual((await preparingResponse.json()).result, { required: false });
    }
    replacement.publish();
    await page.waitForFunction(() => window.voiceFixture.providers.size === 2);
    await waitForVoice(page, fixture.requests);
    await page.getByRole("region", { name: "Voice with Alpha" }).waitFor();
    await page.getByRole("button", { name: "Unmute microphone" }).waitFor();
    await page.getByRole("button", { name: "Unmute speakers" }).waitFor();
    assert.equal(await page.evaluate(() => window.voiceFixture.microphoneTracks.length), capturesBeforeRecovery);
    assert.equal(fixture.requests.filter((request) => request.method === "mobkit/console/voice/open").length, 1);

    await openAgent(page, "Beta");
    await page.getByRole("region", { name: "Voice with Alpha" }).waitFor();
    assert.equal(fixture.requests.filter((request) => request.method === "mobkit/console/voice/open").length, 1);
    await page.getByTestId("nav:roster").click();
    await page.locator('.main > [data-testid="voice-bar"]').waitFor();
    await page.getByRole("region", { name: "Voice with Alpha" }).waitFor();
    await openAgent(page, "Beta");
    await sendText(page, "identity:beta", "A separate text task");
    await page.waitForFunction(() => document.querySelector('[data-testid="chat-composer:identity:beta"]').value === "");
    assert.ok(fixture.requests.some((request) => request.method === "mobkit/console/send" && request.params.identity === "identity:beta"));

    await assertVoiceLayout(page);
    if (process.env.VOICE_SCREENSHOT_DIR) {
      await fs.mkdir(process.env.VOICE_SCREENSHOT_DIR, { recursive: true });
      await page.screenshot({ path: path.join(process.env.VOICE_SCREENSHOT_DIR, "voice-desktop.png") });
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await assertVoiceLayout(page);
    if (process.env.VOICE_SCREENSHOT_DIR) {
      await page.screenshot({ path: path.join(process.env.VOICE_SCREENSHOT_DIR, "voice-mobile.png") });
    }
    await page.setViewportSize({ width: 1280, height: 900 });

    fixture.setContext("identity:alpha", { phase: "failed", reason: "timed_out" });
    await page.getByRole("alert").filter({ hasText: "Context preparation timed out." }).waitFor();
    await waitForVoice(page, fixture.requests);
    assert.equal(await page.getByRole("button", { name: "Unmute microphone" }).isEnabled(), true);
    for (const [name, viewport] of [
      ["desktop", { width: 1280, height: 900 }],
      ["mobile", { width: 390, height: 844 }],
    ]) {
      await page.setViewportSize(viewport);
      await assertVoiceLayout(page);
      if (process.env.VOICE_SCREENSHOT_DIR) {
        await page.screenshot({ path: path.join(process.env.VOICE_SCREENSHOT_DIR, `voice-context-error-${name}.png`) });
      }
    }
    await page.setViewportSize({ width: 1280, height: 900 });

    await page.getByRole("button", { name: "Start voice with Beta" }).click();
    await page.getByRole("region", { name: "Voice with Beta" }).waitFor();
    await waitForVoice(page, fixture.requests);
    const firstClose = fixture.requests.findIndex((request) => request.method === "mobkit/console/voice/close");
    const secondOpen = fixture.requests.findIndex((request) => request.method === "mobkit/console/voice/open" && request.params.identity === "identity:beta");
    assert.ok(firstClose >= 0 && firstClose < secondOpen, "old voice closes before the new voice opens");
    await page.getByRole("button", { name: "End voice conversation" }).click();
    await page.waitForFunction(() => window.voiceFixture.microphoneTracks.every((track) => track.readyState === "ended"));
    assert.ok([...fixture.channels.values()].every((channel) => channel.closed));
    assert.deepEqual(errors, []);
    const openedBeforeAuthLoss = fixture.channels.size;
    const capturesBeforeAuthLoss = await page.evaluate(() => window.voiceFixture.microphoneTracks.length);
    await page.getByRole("button", { name: "Start voice with Beta" }).waitFor();
    fixture.setAvailable(false);
    await page.getByRole("button", { name: "Start voice with Beta" }).click();
    await page.getByRole("alert").filter({ hasText: "authenticate OpenAI" }).waitFor();
    assert.equal(await page.evaluate(() => window.voiceFixture.microphoneTracks.length), capturesBeforeAuthLoss);
    assert.equal(fixture.channels.size, openedBeforeAuthLoss);

    const unauthenticated = await context.newPage();
    const noAuth = await installConsoleFixture(unauthenticated, { available: false });
    await unauthenticated.goto(`${url}/console`);
    await openAgent(unauthenticated, "Alpha");
    assert.equal(await unauthenticated.getByTestId("voice-start").count(), 0);
    assert.equal(await unauthenticated.evaluate(() => window.voiceFixture.microphoneTracks.length), 0);
    assert.ok(!noAuth.requests.some((request) => request.method === "mobkit/console/voice/open"));
    await context.close();
    console.log("Voice browser E2E passed: real WebRTC audio before context release, context acknowledgement/failure, mute, navigation, concurrent text, replacement, cleanup and auth gate.");
  } finally {
    await browser?.close();
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
