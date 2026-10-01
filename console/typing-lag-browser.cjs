#!/usr/bin/env node
"use strict";

// Real-browser typing-lag benchmark.
//
// jsdom does no style, layout or paint, so it cannot see keystroke latency.
// This builds src/perf/typing-lag-harness.tsx with the production console
// build options (the real ConsoleApp, React production build and CSS, fed by
// real-shaped gateway frames paged like the gateway) and drives Chromium:
// for each server history size it opens the docked chat, pages back through
// history like an operator scrolling up until the console's identity log is
// full, returns to the live edge, then types into the composer with real key
// events, first idle and then while a reply streams in.
//
// Latency per keystroke is keydown event.timeStamp (the hardware timestamp)
// to the first task after the next animation frame callback, which runs after
// that frame has been produced. Event Timing entries (>= 16 ms) and long
// tasks are recorded alongside. With --trace a CDP trace is recorded per size
// and renderer main-thread time inside keystroke windows is attributed to
// scripting, style, layout, paint and compositing.
//
// Usage: node typing-lag-browser.cjs [--turns 100,1000,5000] [--keys 40]
//        [--scenarios idle,streaming] [--budget-p95 idle=16,streaming=40]
//        [--trace] [--profile]
//        [--src ../other-tree/console] [--no-fill] [--json out.json]
//        [--session flowforensics|path/to/session.json]

const crypto = require("node:crypto");
const fs = require("node:fs/promises");
const http = require("node:http");
const path = require("node:path");
const { build } = require("esbuild");
const { chromium } = require("playwright");

function arg(name, fallback) {
  const index = process.argv.indexOf(`--${name}`);
  if (index === -1) return fallback;
  const value = process.argv[index + 1];
  return value === undefined || value.startsWith("--") ? true : value;
}

const TURNS = String(arg("turns", "100,1000,5000")).split(",").map(Number);
// Render one persisted session instead of generated turns: "flowforensics"
// (a generated session shaped like a real slow one) or a Session JSON path.
const SESSION = arg("session", null);
const KEYS = Number(arg("keys", "40"));
const SCENARIOS = String(arg("scenarios", "idle,streaming")).split(",");
const TRACE = Boolean(arg("trace", false));
// p95 budget in ms, one number for every scenario or per scenario
// ("idle=16,streaming=40"). MOBKIT_TYPING_LAG_BUDGET overrides the flag.
const BUDGET_SPEC = process.env.MOBKIT_TYPING_LAG_BUDGET || arg("budget-p95", null);
const BUDGET_P95 = BUDGET_SPEC === null || BUDGET_SPEC === true ? null : parseBudget(String(BUDGET_SPEC));

function parseBudget(spec) {
  if (/^[0-9.]+$/.test(spec)) return { default: Number(spec) };
  return Object.fromEntries(spec.split(",").map((part) => {
    const [scenario, ms] = part.split("=");
    return [scenario.trim(), Number(ms)];
  }));
}

function budgetFor(scenario) {
  return BUDGET_P95[scenario] ?? BUDGET_P95.default ?? Infinity;
}
const JSON_OUT = arg("json", null);
const HEADED = Boolean(arg("headed", false));
const FILL = !arg("no-fill", false);
// Open every tool card and disclosure, as an operator reading tool output does.
const EXPAND = Boolean(arg("expand", false));
// Fail when an idle keystroke lays out more than this many layout objects
// (requires --trace). Hardware-independent: a keystroke that reaches the
// transcript lays out tens of thousands of objects, one that stays in the
// composer a handful.
const MAX_LAYOUT_OBJECTS = arg("max-layout-objects", null) === null ? null : Number(arg("max-layout-objects"));
const DPR = Number(arg("dpr", "1"));
// One streamed text_delta frame every STREAM_MS while typing (real token rate).
const STREAM_MS = Number(arg("stream-ms", "25"));
// Console tree to build the harness from (default: this one). Lets the same
// benchmark measure a baseline checkout.
const SRC = path.resolve(String(arg("src", __dirname)));
const PROFILE = Boolean(arg("profile", false));
const MINIFY = !arg("no-minify", false) && !PROFILE;
const TRACE_INVALIDATIONS = Boolean(arg("trace-invalidations", false));
const outDir = path.join(__dirname, ".tmp/typing-lag", crypto.createHash("sha1").update(SRC).digest("hex").slice(0, 8));

async function buildHarness() {
  await fs.mkdir(outDir, { recursive: true });
  if (SRC !== __dirname) {
    // The harness and generator are benchmark files; run the same ones there.
    for (const file of ["typing-lag-harness.tsx", "realistic-transcript.ts", "session-projection.ts"]) {
      await fs.copyFile(path.join(__dirname, "src/perf", file), path.join(SRC, "src/perf", file));
    }
  }
  await build({
    absWorkingDir: SRC,
    entryPoints: [path.join(SRC, "src/perf/typing-lag-harness.tsx")],
    outfile: path.join(outDir, "harness.js"),
    bundle: true,
    format: "iife",
    platform: "browser",
    target: ["es2020"],
    define: { "process.env.NODE_ENV": '"production"', NODE_ENV: '"production"' },
    alias: {
      "@console-core": path.resolve(SRC, "../packages/console-core/src/index.ts"),
      "@console-components": path.resolve(SRC, "../packages/console-components/src/index.ts"),
      "@console-components/styles": path.resolve(SRC, "../packages/console-components/src/styles/index.ts"),
    },
    nodePaths: [path.resolve(SRC, "node_modules")],
    jsx: "automatic",
    keepNames: true,
    minify: MINIFY,
    logLevel: "error",
  });
  await fs.writeFile(
    path.join(outDir, "index.html"),
    `<!doctype html><html lang="en"><head><meta charset="utf-8" /><title>typing lag</title>
<link rel="stylesheet" href="/harness.css" /></head><body><div id="root"></div>
<script src="/harness.js"></script></body></html>`,
  );
}

function serve() {
  const types = { ".html": "text/html", ".js": "application/javascript", ".css": "text/css" };
  const server = http.createServer(async (req, res) => {
    const url = new URL(req.url, "http://x");
    const file = url.pathname === "/" ? "index.html" : path.basename(url.pathname);
    try {
      const bytes = file === "session.json" && SESSION && SESSION !== "flowforensics"
        ? await fs.readFile(path.resolve(String(SESSION)))
        : await fs.readFile(path.join(outDir, file));
      res.writeHead(200, { "content-type": types[path.extname(file)] || "application/octet-stream" });
      res.end(bytes);
    } catch {
      res.writeHead(404);
      res.end();
    }
  });
  return new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server)));
}

// Installed before any page script: Event Timing, long tasks, and the
// keydown-to-after-next-frame probe for composer keystrokes. Trace markers
// (console.timeStamp) bracket each keystroke window for --trace.
function installProbe() {
  window.__lag = { probe: [], events: [], longtasks: [], index: 0 };
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      window.__lag.events.push({ name: entry.name, duration: entry.duration, interactionId: entry.interactionId, start: entry.startTime });
    }
  }).observe({ type: "event", durationThreshold: 16, buffered: true });
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) window.__lag.longtasks.push(entry.duration);
  }).observe({ type: "longtask", buffered: true });
  document.addEventListener(
    "keydown",
    (event) => {
      if (!(event.target instanceof HTMLTextAreaElement)) return;
      const start = event.timeStamp;
      const index = window.__lag.index++;
      console.timeStamp(`lag:start:${index}`);
      requestAnimationFrame(() => {
        const channel = new MessageChannel();
        channel.port1.onmessage = () => {
          window.__lag.probe.push(performance.now() - start);
          console.timeStamp(`lag:end:${index}`);
        };
        channel.port2.postMessage(0);
      });
    },
    true,
  );
}

function percentile(sorted, p) {
  if (sorted.length === 0) return NaN;
  return sorted[Math.min(sorted.length - 1, Math.max(0, Math.ceil(sorted.length * p) - 1))];
}

function summarize(values) {
  const sorted = [...values].sort((a, b) => a - b);
  return { n: sorted.length, p50: percentile(sorted, 0.5), p95: percentile(sorted, 0.95), max: sorted[sorted.length - 1] };
}

// Renderer main-thread slices, attributed by name (self time).
const CATEGORY = {
  scripting: ["FunctionCall", "EventDispatch", "TimerFire", "FireAnimationFrame", "RunMicrotasks", "v8.callFunction", "EvaluateScript", "V8.GC_SCAVENGER_SCAVENGE_PARALLEL", "MinorGC", "MajorGC", "V8.GCScavenger", "BlinkGC.AtomicPhase", "v8.run", "V8.Execute"],
  style: ["UpdateLayoutTree", "RecalculateStyles", "ParseAuthorStyleSheet", "Document::UpdateStyleAndLayoutTree"],
  layout: ["Layout", "LocalFrameView::performLayout", "UpdateLayerTree", "Blink.ForcedStyleAndLayout.UpdateTime", "Document::UpdateStyleAndLayout", "LocalFrameView::UpdateStyleAndLayout", "ResizeObserverController::handleObservations", "IntersectionObserverController::computeIntersections"],
  paint: ["Paint", "PrePaint", "Blink.PrePaint.UpdateTime", "Blink.Paint.UpdateTime", "PaintImage", "LocalFrameView::RunPaintLifecyclePhase"],
  composite: ["Layerize", "Commit", "CompositeLayers", "PaintArtifactCompositor::Update", "Blink.CompositingInputs.UpdateTime", "UpdateLayer"],
};

function analyzeTrace(trace) {
  const events = trace.traceEvents || trace;
  const names = new Map();
  for (const e of events) if (e.ph === "M" && e.name === "thread_name") names.set(`${e.pid}:${e.tid}`, e.args.name);
  const counts = new Map();
  for (const e of events) {
    const key = `${e.pid}:${e.tid}`;
    if (names.get(key) === "CrRendererMain" && e.ph === "X") counts.set(key, (counts.get(key) || 0) + 1);
  }
  const main = [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
  if (!main) return null;
  const slices = events
    .filter((e) => `${e.pid}:${e.tid}` === main && e.ph === "X" && typeof e.dur === "number")
    .sort((a, b) => a.ts - b.ts || b.dur - a.dur);
  const records = [];
  const stack = [];
  for (const e of slices) {
    while (stack.length && stack[stack.length - 1].e.ts + stack[stack.length - 1].e.dur <= e.ts) stack.pop();
    const record = { e, self: e.dur };
    if (stack.length) stack[stack.length - 1].self -= e.dur;
    stack.push(record);
    records.push(record);
  }
  const ranges = [];
  for (const m of events) {
    const message = m.name === "TimeStamp" ? m.args?.data?.message : undefined;
    if (!message || !message.startsWith("lag:")) continue;
    const [, kind, idx] = message.split(":");
    ranges[idx] = ranges[idx] || {};
    ranges[idx][kind] = m.ts;
  }
  const windows = ranges.filter((r) => r && r.start !== undefined && r.end !== undefined);
  // Any main-thread work overlapping a keystroke window delays that
  // keystroke's frame, including a task that started before the keydown.
  const byName = new Map();
  for (const rec of records) {
    const start = rec.e.ts;
    const end = rec.e.ts + rec.e.dur;
    let overlap = 0;
    for (const w of windows) overlap += Math.max(0, Math.min(end, w.end) - Math.max(start, w.start));
    if (overlap <= 0 || rec.self <= 0) continue;
    byName.set(rec.e.name, (byName.get(rec.e.name) || 0) + (rec.self * Math.min(1, overlap / rec.e.dur)) / 1000);
  }
  const totals = { scripting: 0, style: 0, layout: 0, paint: 0, composite: 0, other: 0 };
  for (const [name, ms] of byName) {
    const category = Object.keys(CATEGORY).find((c) => CATEGORY[c].includes(name)) || "other";
    totals[category] += ms;
  }
  const keystrokes = windows.length || 1;
  let maxLayoutObjects = 0;
  for (const e of slices) {
    if (e.name !== "Layout") continue;
    const inside = windows.some((w) => e.ts >= w.start && e.ts <= w.end);
    if (inside) maxLayoutObjects = Math.max(maxLayoutObjects, e.args?.beginData?.totalObjects ?? 0);
  }
  return {
    keystrokes,
    maxLayoutObjects,
    perKey: Object.fromEntries(Object.entries(totals).map(([k, v]) => [k, v / keystrokes])),
    top: [...byName.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([name, ms]) => ({ name, msPerKey: ms / keystrokes })),
  };
}

async function fillHistory(page) {
  // Page back like an operator scrolling up: reveal mounted-but-windowed turns
  // and load older pages until history is exhausted or the log is trimmed.
  for (let step = 0; step < 400; step += 1) {
    const reveal = page.locator('[data-testid^="chat-reveal-earlier"]');
    if (await reveal.count()) {
      await reveal.first().click();
      await page.waitForTimeout(60);
      continue;
    }
    const older = page.locator("button.conv__history:not([disabled])", { hasText: "Load older history" });
    if (!(await older.count())) {
      const loading = page.locator("button.conv__history[disabled]");
      if (await loading.count()) {
        await page.waitForTimeout(100);
        continue;
      }
      break;
    }
    await older.first().click();
    await page.waitForTimeout(120);
  }
}

async function typeKeys(page, textarea) {
  await textarea.click();
  await page.evaluate(() => {
    window.__lag.probe.length = 0;
    window.__lag.events.length = 0;
    window.__lag.longtasks.length = 0;
  });
  const text = "the quick brown fox jumps over the lazy dog and keeps typing ";
  let seed = 7;
  const rand = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
  // Words typed at 60-120 ms per key with 450-900 ms pauses between words,
  // so the composer's debounced draft publish also lands mid-sentence.
  for (let i = 0; i < KEYS; i += 1) {
    const ch = text[i % text.length];
    await page.keyboard.press(ch === " " ? "Space" : ch);
    await page.waitForTimeout(ch === " " ? 450 + rand() * 450 : 60 + rand() * 60);
  }
  await page.waitForTimeout(600);
  const lag = await page.evaluate(() => window.__lag);
  const typed = await textarea.inputValue();
  await textarea.fill("");
  const keyEvents = lag.events.filter((e) => ["keydown", "keypress", "input", "keyup"].includes(e.name));
  const perInteraction = new Map();
  for (const e of keyEvents) {
    const key = e.interactionId || `${e.name}:${e.start}`;
    perInteraction.set(key, Math.max(perInteraction.get(key) || 0, e.duration));
  }
  return {
    typedOk: typed.length >= KEYS,
    latency: summarize(lag.probe),
    samples: lag.probe.map((ms) => Math.round(ms * 10) / 10),
    eventTimingOver16: perInteraction.size,
    eventTimingMax: Math.max(0, ...perInteraction.values()),
    longtasks: summarize(lag.longtasks),
  };
}

async function measureSize(browser, baseUrl, turns) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: DPR });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(installProbe);
  const session = SESSION ? `&session=${SESSION === "flowforensics" ? "flowforensics" : "/session.json"}` : "";
  await page.goto(`${baseUrl}/?turns=${turns}${session}`);
  await page.waitForFunction(() => document.querySelectorAll("[data-chat-turn-index]").length > 0, null, { timeout: 120_000 });
  if (FILL) await fillHistory(page);
  if (EXPAND) {
    for (let pass = 0; pass < 4; pass += 1) {
      const opened = await page.evaluate(() => {
        let count = 0;
        for (const details of document.querySelectorAll(".conv__body details:not([open])")) {
          details.open = true;
          count += 1;
        }
        for (const header of document.querySelectorAll('.conv__body [role="button"][aria-expanded="false"]')) {
          header.click();
          count += 1;
        }
        return count;
      });
      await page.waitForTimeout(400);
      if (!opened) break;
    }
  }
  const textarea = page.locator('textarea[data-testid^="chat-composer"]').first();
  // Back at the live edge, where an operator types.
  await page.evaluate(() => {
    const body = document.querySelector(".conv__body");
    if (body) body.scrollTop = body.scrollHeight;
  });
  await page.waitForTimeout(1500);
  const dom = await page.evaluate(() => {
    const turnNodes = document.querySelectorAll("[data-chat-turn-index]");
    return { elements: document.getElementsByTagName("*").length, mountedTurns: turnNodes.length };
  });
  await textarea.click();
  await page.keyboard.type("warm", { delay: 80 });
  await textarea.fill("");
  await page.waitForTimeout(300);
  const result = { turns, dom, scenarios: {}, errors };
  for (const scenario of SCENARIOS) {
    let stop = null;
    if (scenario === "streaming") {
      await page.evaluate((everyMs) => {
        window.__stopStream = window.__perf.streamReply(everyMs);
      }, STREAM_MS);
      await page.waitForTimeout(1000);
      stop = () => page.evaluate(() => window.__stopStream?.());
    }
    if (TRACE) {
      await browser.startTracing(page, {
        categories: ["devtools.timeline", "disabled-by-default-devtools.timeline", "blink", "v8.execute", "blink.user_timing",
          ...(TRACE_INVALIDATIONS ? ["disabled-by-default-devtools.timeline.invalidationTracking", "disabled-by-default-devtools.timeline.stack"] : [])],
      });
    }
    let cdp = null;
    if (PROFILE) {
      cdp = await context.newCDPSession(page);
      await cdp.send("Profiler.enable");
      await cdp.send("Profiler.setSamplingInterval", { interval: 250 });
      await cdp.send("Profiler.start");
    }
    const measured = await typeKeys(page, textarea);
    if (cdp) {
      const { profile } = await cdp.send("Profiler.stop");
      measured.profile = summarizeProfile(profile);
      await cdp.detach();
    }
    if (TRACE) {
      const buffer = await browser.stopTracing();
      const tracePath = path.join(outDir, `trace-${turns}-${scenario}.json`);
      await fs.writeFile(tracePath, buffer);
      const traceJson = JSON.parse(buffer.toString("utf8"));
      measured.breakdown = analyzeTrace(traceJson);
      if (TRACE_INVALIDATIONS) {
        // Big layouts and the invalidations recorded just before each.
        const events = traceJson.traceEvents || traceJson;
        for (const big of events.filter((e) => e.ph === "X" && e.name === "Layout" && e.dur > 20_000)) {
          const counts = {};
          let stack;
          for (const e of events) {
            if (!/InvalidationTracking/.test(e.name) || e.ts > big.ts || e.ts < big.ts - 300_000) continue;
            const d = e.args?.data || {};
            const key = `${e.name}|${d.reason || ""}|${d.nodeName || ""}`;
            counts[key] = (counts[key] || 0) + 1;
            if (d.stackTrace && !stack) stack = d.stackTrace.slice(0, 8).map((f) => `${f.functionName}:${f.lineNumber}`);
          }
          process.stdout.write(`  big layout ${Math.round(big.dur / 1000)} ms objects=${big.args?.beginData?.totalObjects} dirty=${big.args?.beginData?.dirtyObjects} ${JSON.stringify(counts)} stack=${JSON.stringify(stack)}\n`);
        }
      }
      measured.breakdown.tracePath = tracePath;
    }
    if (stop) await stop();
    result.scenarios[scenario] = measured;
    await page.waitForTimeout(500);
  }
  await context.close();
  return result;
}

// Self and inclusive time per function from a CDP CPU profile.
function summarizeProfile(profile) {
  const byId = new Map(profile.nodes.map((node) => [node.id, node]));
  const parent = new Map();
  for (const node of profile.nodes) for (const child of node.children || []) parent.set(child, node.id);
  const label = (node) => `${node.callFrame.functionName || "(anonymous)"}${node.callFrame.url ? `:${node.callFrame.lineNumber}` : ""}`;
  const self = new Map();
  const inclusive = new Map();
  profile.samples.forEach((id, index) => {
    const dt = (profile.timeDeltas[index] || 0) / 1000;
    const node = byId.get(id);
    self.set(label(node), (self.get(label(node)) || 0) + dt);
    const seen = new Set();
    for (let cursor = id; cursor !== undefined; cursor = parent.get(cursor)) {
      const key = label(byId.get(cursor));
      if (seen.has(key)) continue;
      seen.add(key);
      inclusive.set(key, (inclusive.get(key) || 0) + dt);
    }
  });
  const top = (map, n) => [...map.entries()].sort((a, b) => b[1] - a[1]).slice(0, n).map(([name, ms]) => ({ name, ms: Math.round(ms) }));
  // Who calls the hottest native DOM reads (forced layout, selector scans).
  const callers = new Map();
  profile.samples.forEach((id, index) => {
    const node = byId.get(id);
    const name = node.callFrame.functionName;
    if (!["querySelectorAll", "getBoundingClientRect", "get scrollHeight", "closest", "querySelector"].includes(name)) return;
    const caller = parent.has(id) ? label(byId.get(parent.get(id))) : "(root)";
    const grand = parent.has(parent.get(id)) ? label(byId.get(parent.get(parent.get(id)))) : "";
    const key = `${name} <- ${caller} <- ${grand}`;
    callers.set(key, (callers.get(key) || 0) + (profile.timeDeltas[index] || 0) / 1000);
  });
  return { self: top(self, 25), inclusive: top(inclusive, 45), callers: top(callers, 8) };
}

function fmt(n) {
  return Number.isFinite(n) ? n.toFixed(1) : "-";
}

async function main() {
  await buildHarness();
  const server = await serve();
  const baseUrl = `http://127.0.0.1:${server.address().port}`;
  const browser = await chromium.launch({ headless: !HEADED });
  const results = [];
  try {
    for (const turns of SESSION ? [0] : TURNS) {
      const result = await measureSize(browser, baseUrl, turns);
      results.push(result);
      for (const [scenario, m] of Object.entries(result.scenarios)) {
        const b = m.breakdown?.perKey;
        process.stdout.write(
          `[typing-lag-browser] turns=${turns} ${scenario.padEnd(9)} elements=${result.dom.elements} mounted=${result.dom.mountedTurns} ` +
            `p50=${fmt(m.latency.p50)} p95=${fmt(m.latency.p95)} max=${fmt(m.latency.max)} ms ` +
            `event-timing>=16ms ${m.eventTimingOver16}/${KEYS} longtasks=${m.longtasks.n} (max ${fmt(m.longtasks.max)} ms)` +
            (m.breakdown ? ` layout-objects<=${m.breakdown.maxLayoutObjects}` : "") +
            (b ? ` | per key: script ${fmt(b.scripting)} style ${fmt(b.style)} layout ${fmt(b.layout)} paint ${fmt(b.paint)} composite ${fmt(b.composite)} other ${fmt(b.other)} ms` : "") +
            "\n",
        );
        if (m.profile) {
          process.stdout.write("  profile self:\n");
          for (const t of m.profile.self) process.stdout.write(`    ${String(t.ms).padStart(6)} ms  ${t.name}\n`);
          process.stdout.write("  hot DOM read callers:\n");
          for (const t of m.profile.callers) process.stdout.write(`    ${String(t.ms).padStart(6)} ms  ${t.name}\n`);
          process.stdout.write("  profile inclusive:\n");
          for (const t of m.profile.inclusive) process.stdout.write(`    ${String(t.ms).padStart(6)} ms  ${t.name}\n`);
        }
        if (m.breakdown) {
          for (const t of m.breakdown.top) process.stdout.write(`    ${t.name.padEnd(52)} ${fmt(t.msPerKey)} ms/key\n`);
        }
      }
      if (result.errors.length) process.stdout.write(`  page errors: ${result.errors.slice(0, 3).join(" | ")}\n`);
    }
  } finally {
    await browser.close();
    server.close();
  }
  if (JSON_OUT) await fs.writeFile(JSON_OUT, JSON.stringify(results, null, 2));
  const failures = [];
  if (MAX_LAYOUT_OBJECTS !== null) {
    for (const r of results) {
      const idle = r.scenarios.idle?.breakdown;
      if (!idle) failures.push(`turns=${r.turns}: --max-layout-objects needs --trace and the idle scenario`);
      else if (idle.maxLayoutObjects > MAX_LAYOUT_OBJECTS) {
        failures.push(`turns=${r.turns} idle: a keystroke laid out ${idle.maxLayoutObjects} layout objects > ${MAX_LAYOUT_OBJECTS}; typing is re-laying out the transcript`);
      }
    }
  }
  if (BUDGET_P95 !== null || MAX_LAYOUT_OBJECTS !== null) {
    for (const r of results) {
      for (const [scenario, m] of Object.entries(r.scenarios)) {
        if (BUDGET_P95 === null) continue;
        const budget = budgetFor(scenario);
        if (!(m.latency.p95 <= budget) || !m.typedOk) {
          failures.push(`turns=${r.turns} ${scenario}: p95 ${fmt(m.latency.p95)} ms > budget ${budget} ms (typedOk=${m.typedOk})`);
        }
      }
      if (r.errors.length) failures.push(`turns=${r.turns}: page errors ${r.errors.join(" | ")}`);
    }
    if (failures.length) {
      for (const f of failures) process.stderr.write(`[typing-lag-browser] FAIL ${f}\n`);
      process.exit(1);
    }
    process.stdout.write(`[typing-lag-browser] every size and scenario within budget (p95 ${JSON.stringify(BUDGET_P95)}, layout objects ${MAX_LAYOUT_OBJECTS ?? "unchecked"})\n`);
  }
}

if (require.main === module) {
  main().catch((error) => {
    process.stderr.write(`${error.stack || error.message}\n`);
    process.exit(1);
  });
}

module.exports = { analyzeTrace, installProbe };
