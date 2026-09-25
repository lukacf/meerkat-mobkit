import { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  buildConversationViewState, createHttpConsoleTransport, createMobKitConsoleController,
  mapFramesToTimelineEntries, resolvePanelResponsePhase, type ConsoleFrame, type ConsoleTransportState,
  createPendingApprovalResource, createConsoleContextRecord,
  serializeConsoleContextMessage, type ConsoleContextRecord, type PendingApprovalSnapshot,
} from "@console-core";
import { callConsoleRpc } from "../../packages/console-core/src/network";
import { ConversationPane, ConsoleTransportStatus, ApprovalCard, QuoteContextChips } from "@console-components";
import "@console-components/styles";
import "./shared-conversation-host.css";
import { acceptanceMarkdownUrlPolicy } from "./markdown-url-policy";

const markdownUrlPolicy = acceptanceMarkdownUrlPolicy();

function SharedHost() {
  const [identity, setIdentity] = useState("router:main");
  const [scope, setScope] = useState("fixture-principal-a");
  const [frames, setFrames] = useState<ConsoleFrame[]>([]);
  const [history, setHistory] = useState({ available: false, loading: false });
  const loadOlderRef = useRef<(() => Promise<void>) | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState("");
  const [submitted, setSubmitted] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [twoPanes, setTwoPanes] = useState(false);
  const [showInbox, setShowInbox] = useState(false);
  const [contexts, setContexts] = useState<ConsoleContextRecord[]>([]);
  const [approvals, setApprovals] = useState<PendingApprovalSnapshot>();
  const [transportState, setTransportState] = useState<ConsoleTransportState>({ phase: "connecting", stale: true, freshness: "unknown" });
  const controller = useMemo(() => createMobKitConsoleController({ transport: createHttpConsoleTransport({ baseUrl: location.origin }) }), []);
  const approvalResource = useMemo(() => createPendingApprovalResource({
    scopeKey: scope,
    load: signal => callConsoleRpc(location.origin, "mobkit/gating/pending", {}, undefined, signal),
    decide: (pendingId, decision, signal) => callConsoleRpc(location.origin, "mobkit/gating/decide", {
      pending_id: pendingId, decision, approver_id: "acceptance-operator", reason: "Reviewed evidence",
    }, undefined, signal),
  }), [scope]);
  useEffect(() => {
    const publish = () => setApprovals(approvalResource.getSnapshot());
    const unsubscribe = approvalResource.subscribe(publish); publish();
    return () => { unsubscribe(); approvalResource.dispose(); };
  }, [approvalResource]);
  const activeApprovals = approvals?.scopeKey === scope ? approvals : undefined;

  useEffect(() => {
    const abort = new AbortController(); let unsubscribe: (() => void) | undefined;
    let oldestCursor: string | undefined;
    let exhausted = true;
    let loading = false;
    // Keep page metadata from the same authorized query used by the live
    // controller, without issuing a duplicate initial history request.
    const scopedController = createMobKitConsoleController({ transport: {
      ...controller.transport,
      async queryTimeline(input) {
        const page = await controller.transport.queryTimeline(input);
        if (!abort.signal.aborted && (!oldestCursor || input.before)) {
          oldestCursor = page.frames[0]?.cursor ?? oldestCursor;
          exhausted = page.exhausted === true;
          setHistory({ available: !!oldestCursor && !exhausted, loading });
        }
        return page;
      },
    } });
    loadOlderRef.current = async () => {
      if (loading || exhausted || !oldestCursor || abort.signal.aborted) return;
      loading = true; setHistory({ available: true, loading });
      try {
        const { value: page } = await scopedController.timeline.query({ identity,
          mode: "recent", before: oldestCursor, limit: 200, signal: abort.signal });
        if (abort.signal.aborted) return;
        // The retained live version wins if a concurrent update overlaps this
        // older page. Paging only adds owner frames absent from the transcript.
        setFrames(current => {
          const retained = new Set(current.map(frame => frame.id));
          return [...page.frames.filter(frame => !retained.has(frame.id)), ...current];
        });
      } catch (reason) {
        if (!abort.signal.aborted) setError(String(reason));
      } finally {
        loading = false;
        if (!abort.signal.aborted) setHistory({ available: !!oldestCursor && !exhausted, loading });
      }
    };
    setFrames([]); setHistory({ available: false, loading: false });
    setDraft(""); setContexts([]); setSubmitted(null); setError("");
    void scopedController.timeline.subscribeWithBackfill({ identity, limit: 200, signal: abort.signal, onTransportState: setTransportState }, fact => {
      const frame = fact.value;
      if (frame.event === "snapshot_started" || frame.event === "snapshot_complete") return;
      setFrames(current => {
        const update = frame.event === "frame_updated" && frame.data && typeof frame.data === "object" && "frame" in frame.data
          ? (frame.data as { frame: ConsoleFrame }).frame : frame;
        const index = current.findIndex(item => item.id === update.id);
        return index < 0 ? [...current, update] : current.map((item, i) => i === index ? update : item);
      });
    }).then(value => { if (abort.signal.aborted) value(); else unsubscribe = value; }).catch(reason => {
      if (!abort.signal.aborted) setError(String(reason));
    });
    return () => { abort.abort(); unsubscribe?.(); loadOlderRef.current = null; };
  }, [controller, identity, scope, revision]);

  const entries = useMemo(() => mapFramesToTimelineEntries(null, frames, {
    textMode: "markdown", renderTextDeltas: true, renderInteractionStartsAsUser: true, blobBaseUrl: location.origin,
  }), [frames]);
  const viewState = useMemo(() => buildConversationViewState({ memberId: identity, agentLabel: identity, entries }), [entries, identity]);

  async function send(event: React.FormEvent) {
    event.preventDefault();
    try {
      const accepted = await controller.transport.send({ identity, content: contexts.length ? serializeConsoleContextMessage(draft, contexts) : draft,
        origin: `console:shared:${scope}`, idempotencyKey: crypto.randomUUID() });
      setSubmitted(accepted.input_frame_id ?? null); setDraft(""); setContexts([]); setError("");
    } catch (reason) { setError(String(reason)); }
  }

  return <main className="acceptance-host cc-theme-scope">
    <header>
      <strong>Reusable MobKit conversation</strong>
      <label>Agent <select aria-label="Agent" value={identity} onChange={event => setIdentity(event.target.value)}>
        <option>router:main</option><option>domain:delivery</option>
      </select></label>
      <button type="button" onClick={() => setTwoPanes(value => !value)}>Toggle second pane</button>
      <button type="button" disabled={!history.available || history.loading} onClick={() => void loadOlderRef.current?.()}>Load older history</button>
      <button type="button" onClick={() => { setShowInbox(value => !value); void approvalResource.refresh(); }}>Needs you ({activeApprovals?.status === "ready" ? activeApprovals.requests.length : "?"})</button>
      <button type="button" onClick={() => setScope(value => value === "fixture-principal-a" ? "fixture-principal-b" : "fixture-principal-a")}>Change host scope</button>
      <span data-testid="host-scope">{scope}</span>
    </header>
    <ConsoleTransportStatus state={transportState} onRetry={() => setRevision(value => value + 1)} />
    {error ? <div role="alert">{error}</div> : null}
    {showInbox && activeApprovals ? <aside aria-label="Approval inbox">
      {activeApprovals.requests.map(request => <ApprovalCard key={request.pendingId} request={request}
        resourceStatus={activeApprovals.status} readOnly={activeApprovals.readOnly}
        decision={activeApprovals.decisions[request.pendingId]}
        onDecide={(id, action) => void approvalResource.decide(id, action)} />)}
    </aside> : null}
    <div className="acceptance-panes">
      {[0, ...(twoPanes ? [1] : [])].map(pane => <section key={pane} data-testid={`shared-pane-${pane}`}>
        <ConversationPane viewState={viewState}
          markdownUrlPolicy={markdownUrlPolicy}
          isWorking={resolvePanelResponsePhase({ frames }) !== null}
          approvalSnapshot={activeApprovals}
          onApprovalDecision={(id, action) => void approvalResource.decide(id, action)}
          contextSlot={pane === 0 ? <QuoteContextChips records={contexts} destinationLabel={identity}
            onRemove={id => setContexts(current => current.filter(item => item.id !== id))} /> : undefined}
          onQuoteSelection={quote => {
            try {
              const record = createConsoleContextRecord({ id: crypto.randomUUID(), sourceScope: scope,
                sourceIdentity: identity, messageId: quote.messageId, quote: quote.text, sourceText: quote.sourceText, label: identity });
              setContexts(current => [...current, record]);
              setError("");
            }
            catch (reason) { setError(String(reason)); }
          }}
          viewportKey={{ authority: `${location.origin}/${scope}`, identity, conversation: identity, pane: String(pane) }}
          submittedRowId={submitted ? entries.find(entry => entry.id === submitted || entry.id.startsWith(`${submitted}:`))?.id : null}
          footer={pane === 0 ? <form onSubmit={send}>
            <label>Message to {identity}<textarea aria-label="Message" value={draft} onChange={event => setDraft(event.target.value)} /></label>
            <button type="submit" disabled={!draft.trim()}>Send</button>
          </form> : <p>Second pane reads the same authorized conversation.</p>}
        />
      </section>)}
    </div>
  </main>;
}

createRoot(document.getElementById("root")!).render(<SharedHost />);
