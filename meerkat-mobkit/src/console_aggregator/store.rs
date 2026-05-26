use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::types::{
    AppendDisposition, AppendOutcome, ConsoleCursor, ConsoleFrame, ConsoleFrameSource,
    ConsoleFrameSourceKind, ConsoleFrameStatus, ConsoleTimelineMode, ConsoleTimelinePage,
    ConsoleTimelineQuery, ConsoleTimelineWindowPage, ConsoleTimelineWindowQuery, NewConsoleFrame,
};

pub type ConsoleLogResult<T> = Result<T, ConsoleLogError>;

pub type ConsoleLogError = Box<dyn std::error::Error + Send + Sync>;

#[async_trait::async_trait]
pub trait ConsoleLogStore: Send + Sync {
    async fn append_if_absent(&self, frame: NewConsoleFrame) -> ConsoleLogResult<AppendOutcome>;

    async fn update_frame_status(
        &self,
        frame_id: &str,
        status: ConsoleFrameStatus,
    ) -> ConsoleLogResult<Option<ConsoleFrame>>;

    async fn query_frames(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage>;

    async fn query_windowed_frames(
        &self,
        query: ConsoleTimelineWindowQuery,
    ) -> ConsoleLogResult<ConsoleTimelineWindowPage> {
        if query.mode != ConsoleTimelineMode::Since || query.before.is_some() {
            return Err(std::io::Error::other(
                "console log store must implement query_windowed_frames for v0.4 timeline windows",
            )
            .into());
        }
        let page = self
            .query_frames(ConsoleTimelineQuery {
                identity: query.identity,
                conversation_id: query.conversation_id,
                after: query.after,
                limit: query.limit,
            })
            .await?;
        Ok(ConsoleTimelineWindowPage {
            latest_cursor: page.next_cursor.clone(),
            exhausted: false,
            frames: page.frames,
            next_cursor: page.next_cursor,
        })
    }

    async fn frame_by_dedupe_key(&self, dedupe_key: &str)
    -> ConsoleLogResult<Option<ConsoleFrame>>;

    async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>>;

    async fn clear_frames(&self) -> ConsoleLogResult<()>;

    async fn record_source_watermark(
        &self,
        runtime_key: &str,
        source_kind: ConsoleFrameSourceKind,
        source_cursor: &str,
    ) -> ConsoleLogResult<()>;

    async fn source_watermark(
        &self,
        runtime_key: &str,
        source_kind: ConsoleFrameSourceKind,
    ) -> ConsoleLogResult<Option<String>>;
}

#[derive(Default)]
pub struct InMemoryConsoleLogStore {
    state: Mutex<InMemoryState>,
}

#[derive(Default)]
struct InMemoryState {
    next_seq: u64,
    frames: BTreeMap<u64, ConsoleFrame>,
    dedupe_to_seq: HashMap<String, u64>,
    id_to_seq: HashMap<String, u64>,
    identity_to_seqs: HashMap<String, BTreeSet<u64>>,
    conversation_to_seqs: HashMap<String, BTreeSet<u64>>,
    watermarks: HashMap<(String, String), String>,
}

impl InMemoryConsoleLogStore {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(InMemoryState {
                next_seq: 1,
                frames: BTreeMap::new(),
                dedupe_to_seq: HashMap::new(),
                id_to_seq: HashMap::new(),
                identity_to_seqs: HashMap::new(),
                conversation_to_seqs: HashMap::new(),
                watermarks: HashMap::new(),
            }),
        }
    }
}

#[async_trait::async_trait]
impl ConsoleLogStore for InMemoryConsoleLogStore {
    async fn append_if_absent(&self, frame: NewConsoleFrame) -> ConsoleLogResult<AppendOutcome> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        if let Some(seq) = state.dedupe_to_seq.get(&frame.dedupe_key).copied()
            && let Some(existing) = state.frames.get(&seq)
        {
            return Ok(AppendOutcome {
                disposition: AppendDisposition::Existing,
                frame: existing.clone(),
            });
        }

        let seq = state.next_seq;
        state.next_seq = state.next_seq.saturating_add(1);
        let id = frame
            .id
            .unwrap_or_else(|| stable_frame_id(&frame.dedupe_key));
        let frame = ConsoleFrame {
            id: id.clone(),
            cursor: ConsoleCursor::from_seq(seq),
            dedupe_key: frame.dedupe_key,
            timestamp_ms: frame.timestamp_ms,
            runtime_key: frame.runtime_key,
            identity: frame.identity,
            conversation_id: frame.conversation_id,
            session_id: frame.session_id,
            kind: frame.kind,
            status: frame.status,
            frame_version: 1,
            updated_at_ms: None,
            payload: frame.payload,
            source: frame.source,
            source_event_id: frame.source_event_id,
            interaction_id: frame.interaction_id,
            turn_id: frame.turn_id,
            run_id: frame.run_id,
            parent_frame_id: frame.parent_frame_id,
            caused_by_frame_id: frame.caused_by_frame_id,
        };
        state.dedupe_to_seq.insert(frame.dedupe_key.clone(), seq);
        state.id_to_seq.insert(id, seq);
        state
            .identity_to_seqs
            .entry(frame.identity.clone())
            .or_default()
            .insert(seq);
        if let Some(conversation_id) = frame.conversation_id.as_ref() {
            state
                .conversation_to_seqs
                .entry(conversation_id.clone())
                .or_default()
                .insert(seq);
        }
        state.frames.insert(seq, frame.clone());
        Ok(AppendOutcome {
            disposition: AppendDisposition::Inserted,
            frame,
        })
    }

    async fn update_frame_status(
        &self,
        frame_id: &str,
        status: ConsoleFrameStatus,
    ) -> ConsoleLogResult<Option<ConsoleFrame>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        let Some(seq) = state.id_to_seq.get(frame_id).copied() else {
            return Ok(None);
        };
        let Some(frame) = state.frames.get_mut(&seq) else {
            return Ok(None);
        };
        frame.status = status;
        frame.frame_version = frame.frame_version.saturating_add(1);
        frame.updated_at_ms = Some(current_time_ms());
        Ok(Some(frame.clone()))
    }

    async fn query_frames(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage> {
        let page = self.query_windowed_frames(query.into()).await?;
        Ok(ConsoleTimelinePage {
            frames: page.frames,
            next_cursor: page.next_cursor,
        })
    }

    async fn query_windowed_frames(
        &self,
        query: ConsoleTimelineWindowQuery,
    ) -> ConsoleLogResult<ConsoleTimelineWindowPage> {
        let after_seq = query.after.as_ref().map(cursor_seq).transpose()?;
        let before_seq = query.before.as_ref().map(cursor_seq).transpose()?;
        let limit = normalize_limit(query.limit);
        let scan_limit = limit.saturating_add(1);
        let state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        let frame_matches = |seq: u64, frame: &ConsoleFrame| -> bool {
            if after_seq.is_some_and(|after| seq <= after) {
                return false;
            }
            if before_seq.is_some_and(|before| seq >= before) {
                return false;
            }
            if let Some(identity) = query.identity.as_deref()
                && frame.identity != identity
            {
                return false;
            }
            if let Some(conversation_id) = query.conversation_id.as_deref()
                && frame.conversation_id.as_deref() != Some(conversation_id)
            {
                return false;
            }
            true
        };
        if let (Some(identity), Some(conversation_id)) =
            (query.identity.as_deref(), query.conversation_id.as_deref())
        {
            let identity_seqs = state.identity_to_seqs.get(identity);
            let conversation_seqs = state.conversation_to_seqs.get(conversation_id);
            return match (identity_seqs, conversation_seqs) {
                (Some(left), Some(right)) if left.len() <= right.len() => {
                    Ok(in_memory_window_from_seq_iters(
                        &state,
                        query.mode,
                        (limit, scan_limit),
                        left.iter().copied().filter(|seq| right.contains(seq)),
                        left.iter().rev().copied().filter(|seq| right.contains(seq)),
                        left.iter().rev().copied().filter(|seq| right.contains(seq)),
                        &frame_matches,
                    ))
                }
                (Some(left), Some(right)) => Ok(in_memory_window_from_seq_iters(
                    &state,
                    query.mode,
                    (limit, scan_limit),
                    right.iter().copied().filter(|seq| left.contains(seq)),
                    right.iter().rev().copied().filter(|seq| left.contains(seq)),
                    right.iter().rev().copied().filter(|seq| left.contains(seq)),
                    &frame_matches,
                )),
                _ => Ok(empty_window()),
            };
        }
        if let Some(identity) = query.identity.as_deref() {
            let Some(seqs) = state.identity_to_seqs.get(identity) else {
                return Ok(empty_window());
            };
            return Ok(in_memory_window_from_seq_iters(
                &state,
                query.mode,
                (limit, scan_limit),
                seqs.iter().copied(),
                seqs.iter().rev().copied(),
                seqs.iter().rev().copied(),
                &frame_matches,
            ));
        }
        if let Some(conversation_id) = query.conversation_id.as_deref() {
            let Some(seqs) = state.conversation_to_seqs.get(conversation_id) else {
                return Ok(empty_window());
            };
            return Ok(in_memory_window_from_seq_iters(
                &state,
                query.mode,
                (limit, scan_limit),
                seqs.iter().copied(),
                seqs.iter().rev().copied(),
                seqs.iter().rev().copied(),
                &frame_matches,
            ));
        }
        Ok(in_memory_window_from_seq_iters(
            &state,
            query.mode,
            (limit, scan_limit),
            state.frames.keys().copied(),
            state.frames.keys().rev().copied(),
            state.frames.keys().rev().copied(),
            &frame_matches,
        ))
    }

    async fn frame_by_dedupe_key(
        &self,
        dedupe_key: &str,
    ) -> ConsoleLogResult<Option<ConsoleFrame>> {
        let state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        let Some(seq) = state.dedupe_to_seq.get(dedupe_key).copied() else {
            return Ok(None);
        };
        Ok(state.frames.get(&seq).cloned())
    }

    async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
        let state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        Ok(state
            .frames
            .keys()
            .next_back()
            .copied()
            .map(ConsoleCursor::from_seq))
    }

    async fn clear_frames(&self) -> ConsoleLogResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        state.frames.clear();
        state.dedupe_to_seq.clear();
        state.id_to_seq.clear();
        state.identity_to_seqs.clear();
        state.conversation_to_seqs.clear();
        state.next_seq = 1;
        Ok(())
    }

    async fn record_source_watermark(
        &self,
        runtime_key: &str,
        source_kind: ConsoleFrameSourceKind,
        source_cursor: &str,
    ) -> ConsoleLogResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        state.watermarks.insert(
            (runtime_key.to_string(), source_kind.as_str().to_string()),
            source_cursor.to_string(),
        );
        Ok(())
    }

    async fn source_watermark(
        &self,
        runtime_key: &str,
        source_kind: ConsoleFrameSourceKind,
    ) -> ConsoleLogResult<Option<String>> {
        let state = self
            .state
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        Ok(state
            .watermarks
            .get(&(runtime_key.to_string(), source_kind.as_str().to_string()))
            .cloned())
    }
}

fn empty_window() -> ConsoleTimelineWindowPage {
    ConsoleTimelineWindowPage {
        frames: Vec::new(),
        next_cursor: None,
        latest_cursor: None,
        exhausted: true,
    }
}

fn in_memory_window_from_seq_iters<IForward, IReverse, ILatest, F>(
    state: &InMemoryState,
    mode: ConsoleTimelineMode,
    limits: (usize, usize),
    forward_iter: IForward,
    reverse_iter: IReverse,
    mut latest_iter: ILatest,
    frame_matches: &F,
) -> ConsoleTimelineWindowPage
where
    IForward: Iterator<Item = u64>,
    IReverse: Iterator<Item = u64>,
    ILatest: Iterator<Item = u64>,
    F: Fn(u64, &ConsoleFrame) -> bool,
{
    let (limit, scan_limit) = limits;
    let mut frames = match mode {
        ConsoleTimelineMode::Since => forward_iter
            .filter_map(|seq| {
                let frame = state.frames.get(&seq)?;
                frame_matches(seq, frame).then(|| frame.clone())
            })
            .take(scan_limit)
            .collect::<Vec<_>>(),
        ConsoleTimelineMode::Recent => {
            let mut frames = reverse_iter
                .filter_map(|seq| {
                    let frame = state.frames.get(&seq)?;
                    frame_matches(seq, frame).then(|| frame.clone())
                })
                .take(scan_limit)
                .collect::<Vec<_>>();
            frames.reverse();
            frames
        }
    };
    let exhausted = frames.len() <= limit;
    if frames.len() > limit {
        match mode {
            ConsoleTimelineMode::Since => frames.truncate(limit),
            ConsoleTimelineMode::Recent => {
                frames.remove(0);
            }
        }
    }
    let next_cursor = frames.last().map(|frame| frame.cursor.clone());
    let latest_cursor = match mode {
        ConsoleTimelineMode::Since => latest_iter.find_map(|seq| {
            let frame = state.frames.get(&seq)?;
            frame_matches(seq, frame).then(|| frame.cursor.clone())
        }),
        ConsoleTimelineMode::Recent => next_cursor.clone(),
    };
    ConsoleTimelineWindowPage {
        frames,
        next_cursor,
        latest_cursor,
        exhausted,
    }
}

pub struct SqliteConsoleLogStore {
    conn: Arc<Mutex<Connection>>,
    watermarks: Arc<Mutex<HashMap<(String, String), String>>>,
}

impl SqliteConsoleLogStore {
    pub fn open(path: impl AsRef<Path>) -> ConsoleLogResult<Self> {
        let conn = Connection::open(path).map_err(into_boxed)?;
        Self::from_connection(conn)
    }

    pub fn in_memory() -> ConsoleLogResult<Self> {
        let conn = Connection::open_in_memory().map_err(into_boxed)?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> ConsoleLogResult<Self> {
        initialize_schema(&conn)?;
        let watermarks = load_source_watermarks(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            watermarks: Arc::new(Mutex::new(watermarks)),
        })
    }
}

#[async_trait::async_trait]
impl ConsoleLogStore for SqliteConsoleLogStore {
    async fn append_if_absent(&self, frame: NewConsoleFrame) -> ConsoleLogResult<AppendOutcome> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        if let Some(existing) = select_frame_by_dedupe(&conn, &frame.dedupe_key)? {
            return Ok(AppendOutcome {
                disposition: AppendDisposition::Existing,
                frame: existing,
            });
        }

        let id = frame
            .id
            .clone()
            .unwrap_or_else(|| stable_frame_id(&frame.dedupe_key));
        let payload_json = serde_json::to_string(&frame.payload).map_err(into_boxed)?;
        conn.execute(
            "INSERT INTO console_frames (
                id, dedupe_key, timestamp_ms, runtime_key, identity,
                conversation_id, session_id, kind, status, frame_version, updated_at_ms, payload_json,
                source_kind, source_cursor, source_event_id, interaction_id,
                parent_frame_id, caused_by_frame_id, turn_id, run_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, NULL, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                id,
                frame.dedupe_key,
                frame.timestamp_ms as i64,
                frame.runtime_key,
                frame.identity,
                frame.conversation_id,
                frame.session_id,
                frame.kind,
                frame.status.as_str(),
                payload_json,
                frame.source.kind.as_str(),
                frame.source.source_cursor,
                frame.source_event_id,
                frame.interaction_id,
                frame.parent_frame_id,
                frame.caused_by_frame_id,
                frame.turn_id,
                frame.run_id,
            ],
        )
        .map_err(into_boxed)?;
        let inserted = select_frame_by_dedupe(&conn, &frame.dedupe_key)?
            .ok_or_else(|| boxed_error("inserted console frame was not readable"))?;
        Ok(AppendOutcome {
            disposition: AppendDisposition::Inserted,
            frame: inserted,
        })
    }

    async fn update_frame_status(
        &self,
        frame_id: &str,
        status: ConsoleFrameStatus,
    ) -> ConsoleLogResult<Option<ConsoleFrame>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        conn.execute(
            "UPDATE console_frames SET status = ?1, frame_version = frame_version + 1, updated_at_ms = ?2 WHERE id = ?3",
            params![status.as_str(), current_time_ms() as i64, frame_id],
        )
        .map_err(into_boxed)?;
        select_frame_by_id(&conn, frame_id)
    }

    async fn query_frames(
        &self,
        query: ConsoleTimelineQuery,
    ) -> ConsoleLogResult<ConsoleTimelinePage> {
        let page = self.query_windowed_frames(query.into()).await?;
        Ok(ConsoleTimelinePage {
            frames: page.frames,
            next_cursor: page.next_cursor,
        })
    }

    async fn query_windowed_frames(
        &self,
        query: ConsoleTimelineWindowQuery,
    ) -> ConsoleLogResult<ConsoleTimelineWindowPage> {
        let after_seq = query.after.as_ref().map(cursor_seq_i64).transpose()?;
        let before_seq = query.before.as_ref().map(cursor_seq_i64).transpose()?;
        let limit = normalize_limit(query.limit);
        let scan_limit = limit.saturating_add(1);
        let conn = self
            .conn
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        let mut sql = String::from(
            "SELECT cursor_seq, id, dedupe_key, timestamp_ms, runtime_key, identity,
                    conversation_id, session_id, kind, status, frame_version, updated_at_ms, payload_json,
                    source_kind, source_cursor, source_event_id, interaction_id,
                    parent_frame_id, caused_by_frame_id, turn_id, run_id
             FROM console_frames WHERE cursor_seq > ?1 AND cursor_seq < ?2",
        );
        let mut next_param = 3usize;
        if query.identity.is_some() {
            sql.push_str(" AND identity = ?");
            sql.push_str(&next_param.to_string());
            next_param += 1;
        }
        if query.conversation_id.is_some() {
            sql.push_str(" AND conversation_id = ?");
            sql.push_str(&next_param.to_string());
            next_param += 1;
        }
        match query.mode {
            ConsoleTimelineMode::Since => sql.push_str(" ORDER BY cursor_seq ASC LIMIT ?"),
            ConsoleTimelineMode::Recent => sql.push_str(" ORDER BY cursor_seq DESC LIMIT ?"),
        }
        sql.push_str(&next_param.to_string());

        let after = after_seq.unwrap_or(0);
        let before = before_seq.unwrap_or(i64::MAX);
        let mut values = vec![
            rusqlite::types::Value::Integer(after),
            rusqlite::types::Value::Integer(before),
        ];
        if let Some(identity) = query.identity.as_ref() {
            values.push(rusqlite::types::Value::Text(identity.clone()));
        }
        if let Some(conversation_id) = query.conversation_id.as_ref() {
            values.push(rusqlite::types::Value::Text(conversation_id.clone()));
        }
        values.push(rusqlite::types::Value::Integer(scan_limit as i64));
        let mut frames = query_sql_frames(&conn, &sql, rusqlite::params_from_iter(values))?;
        if query.mode == ConsoleTimelineMode::Recent {
            frames.reverse();
        }
        let exhausted = frames.len() <= limit;
        if frames.len() > limit {
            match query.mode {
                ConsoleTimelineMode::Since => frames.truncate(limit),
                ConsoleTimelineMode::Recent => {
                    frames.remove(0);
                }
            }
        }
        let latest_cursor = latest_matching_cursor(
            &conn,
            after,
            before,
            query.identity.as_deref(),
            query.conversation_id.as_deref(),
        )?;
        let next_cursor = frames.last().map(|frame| frame.cursor.clone());
        Ok(ConsoleTimelineWindowPage {
            frames,
            next_cursor,
            latest_cursor,
            exhausted,
        })
    }

    async fn frame_by_dedupe_key(
        &self,
        dedupe_key: &str,
    ) -> ConsoleLogResult<Option<ConsoleFrame>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        select_frame_by_dedupe(&conn, dedupe_key)
    }

    async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        let seq: Option<i64> = conn
            .query_row(
                "SELECT cursor_seq FROM console_frames ORDER BY cursor_seq DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(into_boxed)?;
        Ok(seq.map(|value| ConsoleCursor::from_seq(value as u64)))
    }

    async fn clear_frames(&self) -> ConsoleLogResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        conn.execute("DELETE FROM console_frames", [])
            .map_err(into_boxed)?;
        conn.execute(
            "DELETE FROM sqlite_sequence WHERE name = 'console_frames'",
            [],
        )
        .ok();
        Ok(())
    }

    async fn record_source_watermark(
        &self,
        runtime_key: &str,
        source_kind: ConsoleFrameSourceKind,
        source_cursor: &str,
    ) -> ConsoleLogResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| boxed_error("console log lock poisoned"))?;
        conn.execute(
            "INSERT INTO console_source_watermarks (
                runtime_key, source_kind, source_cursor, last_ingested_at_ms
            ) VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(runtime_key, source_kind) DO UPDATE SET
                source_cursor = excluded.source_cursor,
                last_ingested_at_ms = excluded.last_ingested_at_ms",
            params![
                runtime_key,
                source_kind.as_str(),
                source_cursor,
                current_time_ms() as i64,
            ],
        )
        .map_err(into_boxed)?;
        self.watermarks
            .lock()
            .map_err(|_| boxed_error("console watermark lock poisoned"))?
            .insert(
                (runtime_key.to_string(), source_kind.as_str().to_string()),
                source_cursor.to_string(),
            );
        Ok(())
    }

    async fn source_watermark(
        &self,
        runtime_key: &str,
        source_kind: ConsoleFrameSourceKind,
    ) -> ConsoleLogResult<Option<String>> {
        let watermarks = self
            .watermarks
            .lock()
            .map_err(|_| boxed_error("console watermark lock poisoned"))?;
        Ok(watermarks
            .get(&(runtime_key.to_string(), source_kind.as_str().to_string()))
            .cloned())
    }
}

fn load_source_watermarks(
    conn: &Connection,
) -> ConsoleLogResult<HashMap<(String, String), String>> {
    let mut stmt = conn
        .prepare(
            "SELECT runtime_key, source_kind, source_cursor
             FROM console_source_watermarks",
        )
        .map_err(into_boxed)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, String>(1)?),
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(into_boxed)?;
    let mut watermarks = HashMap::new();
    for row in rows {
        let (key, cursor) = row.map_err(into_boxed)?;
        watermarks.insert(key, cursor);
    }
    Ok(watermarks)
}

fn initialize_schema(conn: &Connection) -> ConsoleLogResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS console_frames (
            cursor_seq INTEGER PRIMARY KEY AUTOINCREMENT,
            id TEXT NOT NULL UNIQUE,
            dedupe_key TEXT NOT NULL UNIQUE,
            timestamp_ms INTEGER NOT NULL,
            runtime_key TEXT NOT NULL,
            identity TEXT NOT NULL,
            conversation_id TEXT,
            session_id TEXT,
            kind TEXT NOT NULL,
            status TEXT NOT NULL,
            frame_version INTEGER NOT NULL DEFAULT 1,
            updated_at_ms INTEGER,
            payload_json TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            source_cursor TEXT,
            source_event_id TEXT,
            interaction_id TEXT,
            parent_frame_id TEXT,
            caused_by_frame_id TEXT,
            turn_id TEXT,
            run_id TEXT
        );
        CREATE TABLE IF NOT EXISTS console_source_watermarks (
            runtime_key TEXT NOT NULL,
            source_kind TEXT NOT NULL,
            source_cursor TEXT NOT NULL,
            last_ingested_at_ms INTEGER NOT NULL,
            PRIMARY KEY(runtime_key, source_kind)
        );
        CREATE INDEX IF NOT EXISTS idx_console_frames_identity_cursor
            ON console_frames(identity, cursor_seq);
        CREATE INDEX IF NOT EXISTS idx_console_frames_conversation_cursor
            ON console_frames(conversation_id, cursor_seq);",
    )
    .map_err(into_boxed)
}

fn query_sql_frames<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> ConsoleLogResult<Vec<ConsoleFrame>> {
    let mut stmt = conn.prepare(sql).map_err(into_boxed)?;
    let rows = stmt.query_map(params, row_to_frame).map_err(into_boxed)?;
    let mut frames = Vec::new();
    for row in rows {
        frames.push(row.map_err(into_boxed)?);
    }
    Ok(frames)
}

fn select_frame_by_dedupe(
    conn: &Connection,
    dedupe_key: &str,
) -> ConsoleLogResult<Option<ConsoleFrame>> {
    conn.query_row(
        "SELECT cursor_seq, id, dedupe_key, timestamp_ms, runtime_key, identity,
                conversation_id, session_id, kind, status, frame_version, updated_at_ms, payload_json,
                source_kind, source_cursor, source_event_id, interaction_id,
                parent_frame_id, caused_by_frame_id, turn_id, run_id
         FROM console_frames WHERE dedupe_key = ?1",
        params![dedupe_key],
        row_to_frame,
    )
    .optional()
    .map_err(into_boxed)
}

fn select_frame_by_id(conn: &Connection, id: &str) -> ConsoleLogResult<Option<ConsoleFrame>> {
    conn.query_row(
        "SELECT cursor_seq, id, dedupe_key, timestamp_ms, runtime_key, identity,
                conversation_id, session_id, kind, status, frame_version, updated_at_ms, payload_json,
                source_kind, source_cursor, source_event_id, interaction_id,
                parent_frame_id, caused_by_frame_id, turn_id, run_id
         FROM console_frames WHERE id = ?1",
        params![id],
        row_to_frame,
    )
    .optional()
    .map_err(into_boxed)
}

fn latest_matching_cursor(
    conn: &Connection,
    after: i64,
    before: i64,
    identity: Option<&str>,
    conversation_id: Option<&str>,
) -> ConsoleLogResult<Option<ConsoleCursor>> {
    let mut sql = String::from(
        "SELECT cursor_seq FROM console_frames WHERE cursor_seq > ?1 AND cursor_seq < ?2",
    );
    if identity.is_some() {
        sql.push_str(" AND identity = ?3");
    }
    if conversation_id.is_some() {
        sql.push_str(" AND conversation_id = ?");
        let next_param = 3 + usize::from(identity.is_some());
        sql.push_str(&next_param.to_string());
    }
    sql.push_str(" ORDER BY cursor_seq DESC LIMIT 1");

    let mut values = vec![
        rusqlite::types::Value::Integer(after),
        rusqlite::types::Value::Integer(before),
    ];
    if let Some(identity) = identity {
        values.push(rusqlite::types::Value::Text(identity.to_string()));
    }
    if let Some(conversation_id) = conversation_id {
        values.push(rusqlite::types::Value::Text(conversation_id.to_string()));
    }
    let seq: Option<i64> = conn
        .query_row(&sql, rusqlite::params_from_iter(values), |row| row.get(0))
        .optional()
        .map_err(into_boxed)?;
    Ok(seq.map(|value| ConsoleCursor::from_seq(value as u64)))
}

fn row_to_frame(row: &rusqlite::Row<'_>) -> rusqlite::Result<ConsoleFrame> {
    let seq: i64 = row.get(0)?;
    let payload_json: String = row.get(12)?;
    let payload = serde_json::from_str(&payload_json).unwrap_or(serde_json::Value::Null);
    let source_kind: String = row.get(13)?;
    Ok(ConsoleFrame {
        cursor: ConsoleCursor::from_seq(seq as u64),
        id: row.get(1)?,
        dedupe_key: row.get(2)?,
        timestamp_ms: row.get::<_, i64>(3)? as u64,
        runtime_key: row.get(4)?,
        identity: row.get(5)?,
        conversation_id: row.get(6)?,
        session_id: row.get(7)?,
        kind: row.get(8)?,
        status: ConsoleFrameStatus::from_str(row.get::<_, String>(9)?.as_str()),
        frame_version: row.get::<_, i64>(10)? as u64,
        updated_at_ms: row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
        payload,
        source: ConsoleFrameSource {
            kind: ConsoleFrameSourceKind::from_str(&source_kind),
            source_cursor: row.get(14)?,
        },
        source_event_id: row.get(15)?,
        interaction_id: row.get(16)?,
        parent_frame_id: row.get(17)?,
        caused_by_frame_id: row.get(18)?,
        turn_id: row.get(19)?,
        run_id: row.get(20)?,
    })
}

fn normalize_limit(limit: usize) -> usize {
    limit.clamp(1, 1000)
}

fn cursor_seq(cursor: &ConsoleCursor) -> ConsoleLogResult<u64> {
    cursor
        .seq()
        .ok_or_else(|| boxed_error(format!("invalid console cursor: {cursor}")))
}

fn cursor_seq_i64(cursor: &ConsoleCursor) -> ConsoleLogResult<i64> {
    let seq = cursor_seq(cursor)?;
    i64::try_from(seq).map_err(|_| boxed_error(format!("console cursor out of range: {cursor}")))
}

pub(crate) fn stable_frame_id(dedupe_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dedupe_key.as_bytes());
    format!("console-frame-{}", to_hex(&hasher.finalize()))
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn boxed_error(message: impl Into<String>) -> ConsoleLogError {
    Box::new(std::io::Error::other(message.into()))
}

fn into_boxed<E>(error: E) -> ConsoleLogError
where
    E: std::error::Error + Send + Sync + 'static,
{
    Box::new(error)
}

fn current_time_ms() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use serde_json::json;

    use super::*;

    struct LegacyQueryOnlyStore;

    #[async_trait::async_trait]
    impl ConsoleLogStore for LegacyQueryOnlyStore {
        async fn append_if_absent(
            &self,
            _frame: NewConsoleFrame,
        ) -> ConsoleLogResult<AppendOutcome> {
            Err(boxed_error("not implemented for test"))
        }

        async fn update_frame_status(
            &self,
            _frame_id: &str,
            _status: ConsoleFrameStatus,
        ) -> ConsoleLogResult<Option<ConsoleFrame>> {
            Err(boxed_error("not implemented for test"))
        }

        async fn query_frames(
            &self,
            _query: ConsoleTimelineQuery,
        ) -> ConsoleLogResult<ConsoleTimelinePage> {
            Ok(ConsoleTimelinePage {
                frames: Vec::new(),
                next_cursor: None,
            })
        }

        async fn frame_by_dedupe_key(
            &self,
            _dedupe_key: &str,
        ) -> ConsoleLogResult<Option<ConsoleFrame>> {
            Err(boxed_error("not implemented for test"))
        }

        async fn latest_cursor(&self) -> ConsoleLogResult<Option<ConsoleCursor>> {
            Ok(None)
        }

        async fn clear_frames(&self) -> ConsoleLogResult<()> {
            Ok(())
        }

        async fn record_source_watermark(
            &self,
            _runtime_key: &str,
            _source_kind: ConsoleFrameSourceKind,
            _source_cursor: &str,
        ) -> ConsoleLogResult<()> {
            Ok(())
        }

        async fn source_watermark(
            &self,
            _runtime_key: &str,
            _source_kind: ConsoleFrameSourceKind,
        ) -> ConsoleLogResult<Option<String>> {
            Ok(None)
        }
    }

    fn sample_frame(dedupe_key: &str, identity: &str) -> NewConsoleFrame {
        NewConsoleFrame {
            id: None,
            dedupe_key: dedupe_key.to_string(),
            timestamp_ms: 10,
            runtime_key: "runtime-a".to_string(),
            identity: identity.to_string(),
            conversation_id: Some(identity.to_string()),
            session_id: Some("session-1".to_string()),
            kind: "text_delta".to_string(),
            status: ConsoleFrameStatus::Delivered,
            payload: json!({ "delta": "hello" }),
            source: ConsoleFrameSource {
                kind: ConsoleFrameSourceKind::ConsoleEvent,
                source_cursor: None,
            },
            source_event_id: Some(dedupe_key.to_string()),
            interaction_id: None,
            turn_id: None,
            run_id: None,
            parent_frame_id: None,
            caused_by_frame_id: None,
        }
    }

    #[tokio::test]
    async fn legacy_store_default_rejects_v04_window_queries_loudly() {
        let store = LegacyQueryOnlyStore;

        let err = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                mode: ConsoleTimelineMode::Recent,
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect_err("legacy stores must implement recent windows explicitly");
        assert!(
            err.to_string()
                .contains("must implement query_windowed_frames")
        );

        let err = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                mode: ConsoleTimelineMode::Since,
                before: Some(ConsoleCursor::from_seq(10)),
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect_err("legacy stores must implement before windows explicitly");
        assert!(
            err.to_string()
                .contains("must implement query_windowed_frames")
        );

        let page = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                mode: ConsoleTimelineMode::Since,
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("legacy since-only fallback remains source-compatible");
        assert!(page.frames.is_empty());
    }

    #[tokio::test]
    async fn in_memory_log_assigns_monotonic_cursors_and_dedupes() {
        let store = InMemoryConsoleLogStore::new();
        let first = store
            .append_if_absent(sample_frame("event-1", "agent-a"))
            .await
            .expect("append first");
        let duplicate = store
            .append_if_absent(sample_frame("event-1", "agent-a"))
            .await
            .expect("append duplicate");
        let second = store
            .append_if_absent(sample_frame("event-2", "agent-a"))
            .await
            .expect("append second");

        assert_eq!(first.disposition, AppendDisposition::Inserted);
        assert_eq!(duplicate.disposition, AppendDisposition::Existing);
        assert_eq!(first.frame.cursor.seq(), Some(1));
        assert_eq!(second.frame.cursor.seq(), Some(2));
    }

    #[tokio::test]
    async fn sqlite_log_queries_by_identity_and_cursor() {
        let store = SqliteConsoleLogStore::in_memory().expect("sqlite store");
        let first = store
            .append_if_absent(sample_frame("event-1", "agent-a"))
            .await
            .expect("append first");
        store
            .append_if_absent(sample_frame("event-2", "agent-b"))
            .await
            .expect("append second");
        store
            .append_if_absent(sample_frame("event-3", "agent-a"))
            .await
            .expect("append third");

        let page = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                after: Some(first.frame.cursor),
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query");
        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].dedupe_key, "event-3");
    }

    #[tokio::test]
    async fn in_memory_log_queries_recent_window_in_display_order() {
        let store = InMemoryConsoleLogStore::new();
        for index in 1..=6 {
            store
                .append_if_absent(sample_frame(&format!("event-{index}"), "agent-a"))
                .await
                .expect("append frame");
        }

        let page = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                mode: ConsoleTimelineMode::Recent,
                limit: 3,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query recent");
        assert_eq!(
            page.frames
                .iter()
                .map(|frame| frame.dedupe_key.as_str())
                .collect::<Vec<_>>(),
            vec!["event-4", "event-5", "event-6"]
        );
        assert_eq!(
            page.next_cursor.as_ref().and_then(ConsoleCursor::seq),
            Some(6)
        );
        assert_eq!(
            page.latest_cursor.as_ref().and_then(ConsoleCursor::seq),
            Some(6)
        );
        assert!(!page.exhausted);

        let older = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                mode: ConsoleTimelineMode::Recent,
                before: page.frames.first().map(|frame| frame.cursor.clone()),
                limit: 3,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query older");
        assert_eq!(
            older
                .frames
                .iter()
                .map(|frame| frame.dedupe_key.as_str())
                .collect::<Vec<_>>(),
            vec!["event-1", "event-2", "event-3"]
        );
        assert_eq!(
            older.latest_cursor.as_ref().and_then(ConsoleCursor::seq),
            Some(3)
        );
    }

    #[tokio::test]
    async fn in_memory_log_queries_sparse_identity_recent_window_without_global_tail_scan() {
        let store = InMemoryConsoleLogStore::new();
        store
            .append_if_absent(sample_frame("sparse-event", "sparse-agent"))
            .await
            .expect("append sparse frame");
        for index in 1..=25_000 {
            store
                .append_if_absent(sample_frame(&format!("busy-event-{index}"), "busy-agent"))
                .await
                .expect("append busy frame");
        }

        let page = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("sparse-agent".to_string()),
                mode: ConsoleTimelineMode::Recent,
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query sparse recent");

        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].dedupe_key, "sparse-event");
        assert_eq!(
            page.latest_cursor.as_ref().and_then(ConsoleCursor::seq),
            Some(1)
        );
    }

    #[tokio::test]
    async fn sqlite_log_queries_250k_sparse_identity_recent_window_with_index() {
        let store = SqliteConsoleLogStore::in_memory().expect("sqlite store");
        {
            let mut conn = store.conn.lock().expect("sqlite lock");
            let tx = conn.transaction().expect("begin transaction");
            {
                let mut insert = tx
                    .prepare(
                        "INSERT INTO console_frames (
                            id, dedupe_key, timestamp_ms, runtime_key, identity,
                            conversation_id, session_id, kind, status, frame_version, updated_at_ms,
                            payload_json, source_kind, source_cursor, source_event_id,
                            interaction_id, parent_frame_id, caused_by_frame_id, turn_id, run_id
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, NULL, ?10, ?11, NULL, ?12, NULL, NULL, NULL, NULL, NULL)",
                    )
                    .expect("prepare insert");
                insert
                    .execute(rusqlite::params![
                        "sparse-frame",
                        "sparse-event",
                        1_i64,
                        "runtime-a",
                        "sparse-agent",
                        "sparse-agent",
                        "session-sparse",
                        "text_complete",
                        ConsoleFrameStatus::Completed.as_str(),
                        r#"{"text":"still visible"}"#,
                        ConsoleFrameSourceKind::ConsoleEvent.as_str(),
                        "sparse-event",
                    ])
                    .expect("insert sparse frame");
                for index in 2..=250_000_i64 {
                    insert
                        .execute(rusqlite::params![
                            format!("busy-frame-{index}"),
                            format!("busy-event-{index}"),
                            index,
                            "runtime-a",
                            "busy-agent",
                            "busy-agent",
                            "session-busy",
                            "text_delta",
                            ConsoleFrameStatus::Completed.as_str(),
                            format!(r#"{{"delta":{index}}}"#),
                            ConsoleFrameSourceKind::ConsoleEvent.as_str(),
                            format!("busy-event-{index}"),
                        ])
                        .expect("insert busy frame");
                }
            }
            let plan = tx
                .prepare(
                    "EXPLAIN QUERY PLAN
                     SELECT cursor_seq FROM console_frames
                     WHERE cursor_seq > ?1 AND cursor_seq < ?2 AND identity = ?3
                     ORDER BY cursor_seq DESC LIMIT ?4",
                )
                .expect("prepare query plan")
                .query_map(
                    rusqlite::params![0_i64, i64::MAX, "sparse-agent", 11_i64],
                    |row| row.get::<_, String>(3),
                )
                .expect("run query plan")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect query plan")
                .join("\n")
                .to_lowercase();
            assert!(
                plan.contains("idx_console_frames_identity_cursor"),
                "sparse identity recent query should use identity/cursor index; plan was: {plan}"
            );
            tx.commit().expect("commit transaction");
        }

        let page = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("sparse-agent".to_string()),
                mode: ConsoleTimelineMode::Recent,
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query sparse recent");

        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].dedupe_key, "sparse-event");
        assert_eq!(
            page.latest_cursor.as_ref().and_then(ConsoleCursor::seq),
            Some(1)
        );
    }

    #[tokio::test]
    async fn sqlite_log_queries_recent_window_with_before_cursor() {
        let store = SqliteConsoleLogStore::in_memory().expect("sqlite store");
        for index in 1..=6 {
            store
                .append_if_absent(sample_frame(&format!("event-{index}"), "agent-a"))
                .await
                .expect("append frame");
        }

        let page = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                mode: ConsoleTimelineMode::Recent,
                limit: 2,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query recent");
        assert_eq!(
            page.frames
                .iter()
                .map(|frame| frame.dedupe_key.as_str())
                .collect::<Vec<_>>(),
            vec!["event-5", "event-6"]
        );

        let older = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                mode: ConsoleTimelineMode::Recent,
                before: page.frames.first().map(|frame| frame.cursor.clone()),
                limit: 2,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query older");
        assert_eq!(
            older
                .frames
                .iter()
                .map(|frame| frame.dedupe_key.as_str())
                .collect::<Vec<_>>(),
            vec!["event-3", "event-4"]
        );
        assert_eq!(
            older.latest_cursor.as_ref().and_then(ConsoleCursor::seq),
            Some(4)
        );
    }

    #[tokio::test]
    async fn sqlite_log_reports_exhausted_on_exact_size_recent_final_page() {
        let store = SqliteConsoleLogStore::in_memory().expect("sqlite store");
        for index in 1..=400 {
            store
                .append_if_absent(sample_frame(&format!("event-{index}"), "agent-a"))
                .await
                .expect("append frame");
        }

        let first = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                mode: ConsoleTimelineMode::Recent,
                limit: 200,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query recent");
        assert!(!first.exhausted);
        assert_eq!(first.frames[0].dedupe_key, "event-201");

        let older = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                mode: ConsoleTimelineMode::Recent,
                before: first.frames.first().map(|frame| frame.cursor.clone()),
                limit: 200,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query older");
        assert!(older.exhausted);
        assert_eq!(older.frames.len(), 200);
        assert_eq!(older.frames[0].dedupe_key, "event-1");
    }

    #[tokio::test]
    async fn sqlite_log_rejects_out_of_range_console_cursors() {
        let store = SqliteConsoleLogStore::in_memory().expect("sqlite store");
        store
            .append_if_absent(sample_frame("event-1", "agent-a"))
            .await
            .expect("append frame");

        let err = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                after: Some(ConsoleCursor::from("console:9223372036854775808")),
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect_err("oversized after cursor should be rejected");
        assert!(err.to_string().contains("out of range"));

        let err = store
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                before: Some(ConsoleCursor::from("console:9223372036854775808")),
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect_err("oversized before cursor should be rejected");
        assert!(err.to_string().contains("out of range"));
    }

    #[tokio::test]
    async fn sqlite_log_updates_status() {
        let store = SqliteConsoleLogStore::in_memory().expect("sqlite store");
        let first = store
            .append_if_absent(sample_frame("event-1", "agent-a"))
            .await
            .expect("append first");
        let updated = store
            .update_frame_status(&first.frame.id, ConsoleFrameStatus::DeliveryFailed)
            .await
            .expect("update")
            .expect("updated frame");
        assert_eq!(updated.status, ConsoleFrameStatus::DeliveryFailed);
        assert_eq!(updated.frame_version, 2);
        assert!(updated.updated_at_ms.is_some());
    }

    #[tokio::test]
    async fn sqlite_log_records_source_watermarks() {
        let store = SqliteConsoleLogStore::in_memory().expect("sqlite store");
        store
            .record_source_watermark("runtime-a", ConsoleFrameSourceKind::ConsoleEvent, "evt-99")
            .await
            .expect("record watermark");
        let watermark = store
            .source_watermark("runtime-a", ConsoleFrameSourceKind::ConsoleEvent)
            .await
            .expect("read watermark");
        assert_eq!(watermark.as_deref(), Some("evt-99"));
    }

    #[tokio::test]
    async fn sqlite_log_persists_frames_across_handles() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("console.sqlite");
        let store = SqliteConsoleLogStore::open(&path).expect("open first handle");
        store
            .append_if_absent(sample_frame("event-1", "agent-a"))
            .await
            .expect("append frame");
        drop(store);

        let reopened = SqliteConsoleLogStore::open(&path).expect("open second handle");
        let page = reopened
            .query_windowed_frames(ConsoleTimelineWindowQuery {
                identity: Some("agent-a".to_string()),
                limit: 10,
                ..ConsoleTimelineWindowQuery::default()
            })
            .await
            .expect("query frames");
        assert_eq!(page.frames.len(), 1);
        assert_eq!(page.frames[0].dedupe_key, "event-1");
    }
}
