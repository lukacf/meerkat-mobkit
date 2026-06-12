// EchoText — free-text inputs over server-authoritative state.
//
// Every keystroke in the editor round-trips mobkit/mobpacks/apply_operation
// and the document echo re-renders the input. A plainly controlled
// input/textarea breaks under that: React restores the DOM to the (stale)
// value prop after each onChange, snapping the caret to the end, so typing
// mid-string lands characters at the end of the field.
//
// EchoText keeps a local draft while the field is focused — the controlled
// value always matches the DOM, so the caret never moves. While unfocused
// the server document is the only truth: any value change (echo, undo/redo,
// selection switch) syncs the draft. Key usages by the edited entity's id so
// switching selection remounts with clean state.

export function useEchoDraft(value) {
  const [draft, setDraft] = React.useState(value ?? "");
  const focusedRef = React.useRef(false);
  React.useEffect(() => {
    if (!focusedRef.current) setDraft(value ?? "");
  }, [value]);
  return { draft, setDraft, focusedRef };
}

function echoProps({ value, onChangeText, onFocus, onBlur, ...rest }, draftState) {
  const { draft, setDraft, focusedRef } = draftState;
  return {
    ...rest,
    value: draft,
    onFocus: (e) => {
      focusedRef.current = true;
      if (onFocus) onFocus(e);
    },
    onBlur: (e) => {
      focusedRef.current = false;
      if (onBlur) onBlur(e);
    },
    onChange: (e) => {
      setDraft(e.target.value);
      if (onChangeText) onChangeText(e.target.value);
    },
  };
}

export function EchoInput(props) {
  const draftState = useEchoDraft(props.value);
  return <input {...echoProps(props, draftState)} />;
}

export function EchoTextArea(props) {
  const draftState = useEchoDraft(props.value);
  return <textarea {...echoProps(props, draftState)} />;
}
