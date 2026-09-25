use meerkat_mobkit::{
    AppendOutcome, ConsoleCursor, ConsoleFrame, ConsoleFrameSourceKind, ConsoleFrameStatus,
    ConsoleLogResult, ConsoleLogStore, ConsoleTimelinePage, ConsoleTimelineQuery,
    ConsoleTimelineQueryError, ConsoleTimelineWindowPage, ConsoleTimelineWindowQuery,
    NewConsoleFrame,
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

pub struct FaultStore {
    inner: Arc<dyn ConsoleLogStore>,
    fault: AtomicU8,
}

impl FaultStore {
    pub fn new(inner: Arc<dyn ConsoleLogStore>) -> Self {
        Self {
            inner,
            fault: AtomicU8::new(0),
        }
    }
    pub fn set_fault(&self, name: &str) -> Result<(), &'static str> {
        let value = match name {
            "none" => 0,
            "read" => 1,
            "latest" => 2,
            "progress" => 3,
            "expired" => 4,
            _ => return Err("unknown fault"),
        };
        self.fault.store(value, Ordering::SeqCst);
        Ok(())
    }
}

#[async_trait::async_trait]
impl ConsoleLogStore for FaultStore {
    async fn append_if_absent(&self, frame: NewConsoleFrame) -> ConsoleLogResult<AppendOutcome> {
        self.inner.append_if_absent(frame).await
    }
    async fn update_frame_status(
        &self,
        id: &str,
        status: ConsoleFrameStatus,
    ) -> ConsoleLogResult<Option<ConsoleFrame>> {
        self.inner.update_frame_status(id, status).await
    }
    async fn query_frames(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage> {
        self.inner.query_frames(query).await
    }
    async fn query_windowed_frames(
        &self,
        query: ConsoleTimelineWindowQuery,
    ) -> ConsoleLogResult<ConsoleTimelineWindowPage> {
        match self.fault.load(Ordering::SeqCst) {
            1 => Err(std::io::Error::other(
                "private fixture DSN replay_unavailable is untyped text",
            )
            .into()),
            3 => Ok(ConsoleTimelineWindowPage {
                frames: vec![],
                next_cursor: query.after,
                latest_cursor: self.inner.latest_cursor().await?,
                exhausted: false,
            }),
            4 if query.after.is_some() => {
                Err(Box::new(ConsoleTimelineQueryError::ReplayUnavailable {
                    requested_cursor: query.after,
                    latest_cursor: self.inner.latest_cursor().await?,
                }))
            }
            _ => self.inner.query_windowed_frames(query).await,
        }
    }
    async fn frame_by_dedupe_key(&self, key: &str) -> ConsoleLogResult<Option<ConsoleFrame>> {
        self.inner.frame_by_dedupe_key(key).await
    }
    async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
        if self.fault.load(Ordering::SeqCst) == 2 {
            return Err(std::io::Error::other("private fixture latest-cursor failure").into());
        }
        self.inner.latest_cursor().await
    }
    async fn clear_frames(&self) -> ConsoleLogResult<()> {
        self.inner.clear_frames().await
    }
    async fn record_source_watermark(
        &self,
        runtime: &str,
        kind: ConsoleFrameSourceKind,
        cursor: &str,
    ) -> ConsoleLogResult<()> {
        self.inner
            .record_source_watermark(runtime, kind, cursor)
            .await
    }
    async fn source_watermark(
        &self,
        runtime: &str,
        kind: ConsoleFrameSourceKind,
    ) -> ConsoleLogResult<Option<String>> {
        self.inner.source_watermark(runtime, kind).await
    }
}
