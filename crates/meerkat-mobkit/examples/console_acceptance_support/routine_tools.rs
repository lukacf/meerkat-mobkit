//! Actual filesystem tool dispatch for routine-tool presentation acceptance.

use std::path::{Component, Path};
use std::sync::{Arc, Mutex};

use meerkat_core::ops::ToolDispatchOutcome;
use meerkat_core::{AgentToolDispatcher, ToolCallView, ToolDef, ToolError, ToolResult};
use meerkat_mobkit::identity_first::{
    AgentBuildContext, AgentBuildDraft, AgentCustomizer, CustomizerError, DurableAgentSpec,
    LocalExternalToolOverlay,
};
use serde_json::{Value, json};
use tokio::sync::Notify;

pub const NOTES: &str = "  Release notes\nKeep both spaces and this final newline.\n";
pub const LATE_REVIEW: &str = "Late review: all release artifacts match.\n";

#[derive(Default)]
struct Progress {
    entered: bool,
    released: bool,
}

pub struct RoutineTools {
    root: tempfile::TempDir,
    progress: Mutex<Progress>,
    release: Notify,
}

impl RoutineTools {
    pub fn new() -> std::io::Result<Self> {
        let root = tempfile::tempdir()?;
        std::fs::write(root.path().join("release-notes.txt"), NOTES)?;
        std::fs::write(root.path().join("late-review.txt"), LATE_REVIEW)?;
        Ok(Self {
            root,
            progress: Mutex::default(),
            release: Notify::new(),
        })
    }

    pub fn control(&self, action: &str) -> Result<Value, String> {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match action {
            "status" => {}
            "release" => {
                progress.released = true;
                self.release.notify_waiters();
            }
            _ => return Err("routine tool action must be status or release".into()),
        }
        Ok(json!({"entered": progress.entered, "released": progress.released}))
    }

    async fn wait_for_release(&self) {
        loop {
            let notified = self.release.notified();
            {
                let mut progress = self
                    .progress
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                progress.entered = true;
                if progress.released {
                    return;
                }
            }
            notified.await;
        }
    }
}

pub struct RoutineCustomizer(pub Arc<RoutineTools>);

#[async_trait::async_trait]
impl AgentCustomizer for RoutineCustomizer {
    async fn customize_build(
        &self,
        _: &AgentBuildContext,
        _: &DurableAgentSpec,
        draft: &mut AgentBuildDraft,
    ) -> Result<(), CustomizerError> {
        draft.local_external_tools = LocalExternalToolOverlay::new(self.0.clone());
        Ok(())
    }
}

#[async_trait::async_trait]
impl AgentToolDispatcher for RoutineTools {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        [
            ToolDef { name: "read_file".into(), description: "Read one file from the acceptance workspace.".into(), input_schema: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}), provenance: None },
            ToolDef { name: "list_files".into(), description: "List the acceptance workspace files.".into(), input_schema: json!({"type":"object","properties":{},"additionalProperties":false}), provenance: None },
        ].into_iter().map(Arc::new).collect::<Vec<_>>().into()
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        let (text, is_error) = match call.name {
            "list_files" => {
                let mut names = std::fs::read_dir(self.root.path())
                    .expect("fixture directory")
                    .map(|entry| {
                        entry
                            .expect("fixture entry")
                            .file_name()
                            .to_string_lossy()
                            .into_owned()
                    })
                    .collect::<Vec<_>>();
                names.sort();
                (format!("{}\n", names.join("\n")), false)
            }
            "read_file" => {
                let args: Value =
                    serde_json::from_str(call.args.get()).expect("valid fixture arguments");
                let name = args.get("path").and_then(Value::as_str).unwrap_or("");
                let components = Path::new(name).components().collect::<Vec<_>>();
                if components.len() != 1 || !matches!(components[0], Component::Normal(_)) {
                    ("Only one workspace filename is accepted.".to_string(), true)
                } else {
                    if name == "late-review.txt" {
                        self.wait_for_release().await;
                    }
                    match std::fs::read_to_string(self.root.path().join(name)) {
                        Ok(text) => (text, false),
                        Err(error) => (format!("Cannot read {name}: {:?}", error.kind()), true),
                    }
                }
            }
            _ => return Err(ToolError::not_found(call.name)),
        };
        Ok(ToolResult::new(call.id.to_string(), text, is_error).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn dispatch(
        tools: &RoutineTools,
        id: &str,
        name: &str,
        args: Value,
    ) -> ToolDispatchOutcome {
        let args = serde_json::value::RawValue::from_string(args.to_string()).unwrap();
        tools
            .dispatch(ToolCallView {
                id,
                name,
                args: &args,
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn routine_files_preserve_exact_bytes_and_real_missing_file_error() {
        let tools = RoutineTools::new().unwrap();
        let notes = dispatch(
            &tools,
            "notes",
            "read_file",
            json!({"path":"release-notes.txt"}),
        )
        .await;
        assert_eq!(notes.result.text_content(), NOTES);
        assert!(!notes.result.is_error);
        let missing = dispatch(
            &tools,
            "missing",
            "read_file",
            json!({"path":"missing.txt"}),
        )
        .await;
        assert!(missing.result.is_error);
        assert_eq!(
            missing.result.text_content(),
            "Cannot read missing.txt: NotFound"
        );
        let files = dispatch(&tools, "files", "list_files", json!({})).await;
        assert_eq!(
            files.result.text_content(),
            "late-review.txt\nrelease-notes.txt\n"
        );
    }

    #[tokio::test]
    async fn routine_late_read_has_no_result_until_explicit_release() {
        let tools = Arc::new(RoutineTools::new().unwrap());
        let dispatched = tools.clone();
        let task = tokio::spawn(async move {
            dispatch(
                &dispatched,
                "late",
                "read_file",
                json!({"path":"late-review.txt"}),
            )
            .await
        });
        while tools.control("status").unwrap()["entered"] != true {
            tokio::task::yield_now().await;
        }
        assert!(!task.is_finished());
        tools.control("release").unwrap();
        let result = task.await.unwrap();
        assert_eq!(result.result.text_content(), LATE_REVIEW);
        assert!(!result.result.is_error);
    }
}
