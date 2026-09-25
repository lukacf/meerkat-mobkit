import type { ConsoleContextRecord } from "../../../console-core/src/context-record";

export interface QuoteContextChipsProps {
  records: readonly ConsoleContextRecord[];
  destinationLabel: string;
  onRemove?: (id: string) => void;
  onReorder?: (id: string, direction: "up" | "down") => void;
}

export function QuoteContextChips({ records, destinationLabel, onRemove, onReorder }: QuoteContextChipsProps) {
  if (!records.length) return null;
  return <section className="cc-context-chips" aria-label={`Quoted context for ${destinationLabel}`}>
    <span className="cc-context-chips__destination">Quoted context for {destinationLabel}</span>
    <ul>{records.map((record, index) => <li key={record.id} className="cc-context-chip">
      <details>
        <summary>{record.label}</summary>
        <blockquote>{record.quote}</blockquote>
        <small>User-provided snapshot{record.sourceRange ? "" : "; original source range unavailable"}</small>
      </details>
      {onReorder ? <>
        <button type="button" disabled={index === 0} onClick={() => onReorder(record.id, "up")} aria-label={`Move quote from ${record.label} earlier`}>Up</button>
        <button type="button" disabled={index === records.length - 1} onClick={() => onReorder(record.id, "down")} aria-label={`Move quote from ${record.label} later`}>Down</button>
      </> : null}
      {onRemove ? <button type="button" onClick={() => onRemove(record.id)} aria-label={`Remove quote from ${record.label}`}>Remove</button> : null}
    </li>)}</ul>
  </section>;
}
