/* global React */
// Deploy plan trace, validate sheet, source drawer.

function DrySim({ open, onClose, onActiveStep, runKey, document, plan }) {
  const traceState = React.useMemo(() =>
    window.MobKitFlowController.deployPlanTraceState(document, plan),
    [document, plan]);
  const [idx, setIdx] = React.useState(0);
  const bodyRef = React.useRef(null);

  React.useEffect(() => {
    if (!open) return;
    setIdx(0);
  }, [open, runKey]);

  React.useEffect(() => {
    if (!open) { onActiveStep(null); return; }
    onActiveStep(traceState.steps[idx]?.node || null);
    if (bodyRef.current) {
      const el = bodyRef.current.querySelector(`[data-step="${idx}"]`);
      if (el) el.scrollIntoView({ block: "nearest", behavior: "smooth" });
    }
  }, [idx, open, traceState.steps]);

  if (!open) return null;

  return (
    <div className="drysim">
      <div className="drysim__head">
        <div>
          <div className="drysim__title"><span className="accent">{traceState.eyebrow}</span> · {traceState.title}</div>
          <div className="drysim__sub">{traceState.subtitle}</div>
        </div>
        <div className="row">
          <button className="btn btn--sm" onClick={() => setIdx(0)}>{traceState.firstLabel}</button>
          <button className="btn btn--ghost btn--sm" onClick={onClose}>{traceState.closeLabel}</button>
        </div>
      </div>
      <div className="drysim__body" ref={bodyRef}>
        {traceState.steps.map((s, i) => (
          <div
            key={i}
            data-step={i}
            className={"drysim__step" + (i === idx ? " is-current" : "") + (i > idx ? " is-pending" : "")}
          >
            <div className="g" />
            <div>
              <div className="head">{s.head}</div>
              <div className="body">{s.body}</div>
            </div>
          </div>
        ))}
      </div>
      <div className="drysim__foot">
        <div className="row row--between" style={{ width: "100%" }}>
          <span className="muted">{traceState.packLabel ? `${traceState.packLabel} · ` : ""}{traceState.stepLabel} {idx + 1} / {traceState.steps.length}</span>
          <div className="row">
            <button className="btn btn--sm" onClick={() => setIdx(i => Math.max(0, i - 1))}>{traceState.previousLabel}</button>
            <button className="btn btn--sm" onClick={() => setIdx(i => Math.min(traceState.steps.length - 1, i + 1))}>{traceState.nextLabel}</button>
          </div>
        </div>
      </div>
    </div>
  );
}

function ValidateSheet({ open, onClose, onPublish, onDeployPlan, onDeployRun, results, stage }) {
  if (!open) return null;
  const sheetState = window.MobKitFlowController.validationSheetState(results, { stage });
  return (
    <div className="validate">
      <div className="validate__head">
        <div>
          <div className="inspector__eyebrow">{sheetState.eyebrow}</div>
          <div className="inspector__title">{sheetState.title}</div>
        </div>
        <div className="row">
          <button className="btn btn--primary btn--sm" onClick={onPublish} disabled={sheetState.actionsDisabled}>{sheetState.publishLabel}</button>
          <button className="btn btn--ghost btn--sm" onClick={onDeployPlan} disabled={sheetState.actionsDisabled}>{sheetState.deployPlanLabel}</button>
          <button className="btn btn--primary btn--sm" onClick={onDeployRun} disabled={sheetState.actionsDisabled}>{sheetState.deployLabel}</button>
          <button className="btn btn--ghost btn--sm" onClick={onClose}>{sheetState.closeLabel}</button>
        </div>
      </div>
      <div className="validate__body">
        {sheetState.rows.map((r, i) => (
          <div key={i} className={"validate__row is-" + r.kind}>
            <span className="glyph">{r.glyph}</span>
            <div>
              <div className="head">{r.head}</div>
              <div className="sub">{r.sub}</div>
            </div>
            <span className="meta">{r.meta}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function SourceCodePanel({ state, busy = false, compact = false, sourceView = null }) {
  const editorState = window.MobKitFlowController.sourceEditorState(state, { busy, compact, sourceView });
  if (editorState.showLoading) {
    return <pre className={editorState.bodyClass} role="textbox" aria-readonly="true">{editorState.loadingText}</pre>;
  }
  return (
    <pre
      className={editorState.bodyClass}
      role="textbox"
      aria-readonly="true"
      dangerouslySetInnerHTML={{ __html: highlightToml(editorState.source) }}
    />
  );
}

function SourceDrawer({ open, onClose, state, sourceView = null }) {
  if (!open) return null;
  const editorState = window.MobKitFlowController.sourceEditorState(state, { sourceView });
  return (
    <div className="source-drawer">
      <div className="source-drawer__head">
        <div>
          <div className="inspector__eyebrow">{editorState.drawerEyebrow}</div>
          <div className="inspector__id">{editorState.sourceLabel}</div>
          {editorState.validationSource && <div className="inspector__id">{editorState.validationSource}</div>}
        </div>
        <div className="row">
          <button className="btn btn--sm" onClick={() => navigator.clipboard?.writeText(editorState.source)}>{editorState.copyLabel}</button>
          <button className="btn btn--ghost btn--sm" onClick={onClose}>{editorState.closeLabel}</button>
        </div>
      </div>
      <SourceCodePanel state={state} sourceView={sourceView} />
    </div>
  );
}

function InlineSourceEditor({ open, onClose, state, busy = false, surface = "basic", sourceView = null }) {
  if (!open) return null;
  const editorState = window.MobKitFlowController.sourceEditorState(state, { busy, compact: true, sourceView });
  return (
    <div className={"bld-toml bld-toml--" + surface} onMouseDown={e => e.stopPropagation()}>
      <div className="bld-toml__head">
        <div>
          <div>{editorState.inlineTitle}</div>
          <div className="bld-toml__hint">{editorState.sourceLabel}</div>
          {editorState.validationSource && <div className="bld-toml__hint">{editorState.validationSource}</div>}
        </div>
        <div className="row">
          <button className="btn btn--sm" onClick={() => navigator.clipboard?.writeText(editorState.source)} disabled={editorState.copyDisabled}>{editorState.copyLabel}</button>
          <button className="btn btn--ghost btn--sm" onClick={onClose}>{editorState.closeLabel}</button>
        </div>
      </div>
      <SourceCodePanel state={state} busy={busy} compact sourceView={sourceView} />
    </div>
  );
}

function highlightToml(s) {
  return s
    .replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    .replace(/^(\s*#.*)$/gm, '<span class="toml-comment">$1</span>')
    .replace(/^(\s*)(\[[^\]]+\])/gm, '$1<span class="toml-table">$2</span>')
    .replace(/^(\s*)([A-Za-z_][\w-]*)(\s*=)/gm, '$1<span class="toml-key">$2</span>$3');
}

window.DrySim = DrySim;
window.ValidateSheet = ValidateSheet;
window.SourceDrawer = SourceDrawer;
window.InlineSourceEditor = InlineSourceEditor;
