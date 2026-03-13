//! Memory subsystem — Elephant-backed store adapter and query execution.

use super::*;

impl ElephantMemoryStoreAdapter {
    pub(super) fn from_config(
        config: &ElephantMemoryBackendConfig,
    ) -> Result<Self, ElephantMemoryStoreError> {
        let endpoint = config.endpoint.trim();
        if endpoint.is_empty() {
            return Err(ElephantMemoryStoreError::InvalidConfig(
                "memory backend endpoint must not be empty".to_string(),
            ));
        }
        let state_path = config.state_path.trim();
        if state_path.is_empty() {
            return Err(ElephantMemoryStoreError::InvalidConfig(
                "memory backend state_path must not be empty".to_string(),
            ));
        }
        Ok(Self {
            endpoint: endpoint.to_string(),
            state_path: PathBuf::from(state_path),
        })
    }

    fn ensure_remote_health(&self) -> Result<(), ElephantMemoryStoreError> {
        let health_url = format!("{}/v1/health", self.endpoint.trim_end_matches('/'));
        let parsed = parse_http_url(&health_url)?;
        let authority = format!("{}:{}", parsed.host, parsed.port);
        let mut addrs = authority.to_socket_addrs().map_err(|err| {
            ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck resolve failed for '{health_url}': {err}"
            ))
        })?;
        let addr = addrs.next().ok_or_else(|| {
            ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck resolve failed for '{health_url}': no socket addresses"
            ))
        })?;
        let mut stream =
            TcpStream::connect_timeout(&addr, ELEPHANT_HEALTHCHECK_TIMEOUT).map_err(|err| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck connect failed for '{health_url}': {err}"
                ))
            })?;
        stream
            .set_read_timeout(Some(ELEPHANT_HEALTHCHECK_TIMEOUT))
            .map_err(|err| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck timeout setup failed for '{health_url}': {err}"
                ))
            })?;
        stream
            .set_write_timeout(Some(ELEPHANT_HEALTHCHECK_TIMEOUT))
            .map_err(|err| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck timeout setup failed for '{health_url}': {err}"
                ))
            })?;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            parsed.path, parsed.host
        );
        stream.write_all(request.as_bytes()).map_err(|err| {
            ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck write failed for '{health_url}': {err}"
            ))
        })?;
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        let bytes_read = reader.read_line(&mut status_line).map_err(|err| {
            ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck read failed for '{health_url}': {err}"
            ))
        })?;
        if bytes_read == 0 {
            return Err(ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck read failed for '{health_url}': empty response"
            )));
        }
        let status_code = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or_else(|| {
                ElephantMemoryStoreError::ExternalCallFailed(format!(
                    "healthcheck parse failed for '{health_url}': invalid status line '{}'",
                    status_line.trim()
                ))
            })?;
        if (200..300).contains(&status_code) {
            Ok(())
        } else {
            Err(ElephantMemoryStoreError::ExternalCallFailed(format!(
                "healthcheck status failed for '{health_url}': HTTP {status_code}"
            )))
        }
    }

    pub(super) fn read_state(&self) -> Result<PersistedMemoryState, ElephantMemoryStoreError> {
        self.ensure_remote_health()?;
        if !self.state_path.exists() {
            return Ok(PersistedMemoryState::default());
        }
        let bytes = fs::read(&self.state_path)
            .map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        serde_json::from_slice::<PersistedMemoryState>(&bytes)
            .map_err(|err| ElephantMemoryStoreError::InvalidStoreData(err.to_string()))
    }

    fn write_state(&self, state: &PersistedMemoryState) -> Result<(), ElephantMemoryStoreError> {
        self.ensure_remote_health()?;
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        }
        let tmp_path = self.state_path.with_extension("tmp");
        let json = serde_json::to_vec_pretty(state)
            .map_err(|err| ElephantMemoryStoreError::Serialize(err.to_string()))?;
        fs::write(&tmp_path, json).map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        fs::rename(&tmp_path, &self.state_path)
            .map_err(|err| ElephantMemoryStoreError::Io(err.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedHttpUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_url(url: &str) -> Result<ParsedHttpUrl, ElephantMemoryStoreError> {
    let trimmed = url.trim();
    let without_scheme = trimmed.strip_prefix("http://").ok_or_else(|| {
        ElephantMemoryStoreError::InvalidConfig(format!(
            "memory backend endpoint must start with http:// (got '{trimmed}')"
        ))
    })?;
    if without_scheme.is_empty() {
        return Err(ElephantMemoryStoreError::InvalidConfig(
            "memory backend endpoint host must not be empty".to_string(),
        ));
    }
    let (authority, path_suffix) = without_scheme
        .split_once('/')
        .map(|(left, right)| (left, format!("/{right}")))
        .unwrap_or((without_scheme, "/".to_string()));
    if authority.is_empty() {
        return Err(ElephantMemoryStoreError::InvalidConfig(
            "memory backend endpoint host must not be empty".to_string(),
        ));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, raw_port))
            if !host.is_empty() && raw_port.chars().all(|c| c.is_ascii_digit()) =>
        {
            let parsed = raw_port.parse::<u16>().map_err(|_| {
                ElephantMemoryStoreError::InvalidConfig(format!(
                    "memory backend endpoint port is invalid in '{trimmed}'"
                ))
            })?;
            (host.to_string(), parsed)
        }
        _ => (authority.to_string(), 80_u16),
    };
    if host.is_empty() {
        return Err(ElephantMemoryStoreError::InvalidConfig(
            "memory backend endpoint host must not be empty".to_string(),
        ));
    }
    Ok(ParsedHttpUrl {
        host,
        port,
        path: path_suffix,
    })
}

impl MobkitRuntimeHandle {
    fn next_memory_sequence(&mut self) -> u64 {
        Self::next_sequence(&mut self.memory_sequence)
    }

    pub(super) fn canonical_memory_token(raw: &str) -> Option<String> {
        let token = raw.trim().to_ascii_lowercase();
        if token.is_empty() { None } else { Some(token) }
    }

    pub(super) fn canonical_memory_store(raw: &str) -> Option<String> {
        let store = Self::canonical_memory_token(raw)?;
        if MEMORY_SUPPORTED_STORES.contains(&store.as_str()) {
            Some(store)
        } else {
            None
        }
    }

    fn default_memory_store() -> String {
        "knowledge_graph".to_string()
    }

    pub(super) fn memory_conflict_for_reference(
        &self,
        entity: Option<&str>,
        topic: Option<&str>,
    ) -> Option<MemoryConflictSignal> {
        let canonical_entity = entity.and_then(Self::canonical_memory_token);
        let canonical_topic = topic.and_then(Self::canonical_memory_token);
        match (canonical_entity, canonical_topic) {
            (Some(entity), Some(topic)) => self
                .memory_conflicts
                .values()
                .find(|signal| signal.entity == entity && signal.topic == topic)
                .cloned(),
            (Some(entity), None) => self
                .memory_conflicts
                .values()
                .find(|signal| signal.entity == entity)
                .cloned(),
            (None, Some(topic)) => self
                .memory_conflicts
                .values()
                .find(|signal| signal.topic == topic)
                .cloned(),
            (None, None) => None,
        }
    }
    pub fn memory_stores(&self) -> Vec<MemoryStoreInfo> {
        MEMORY_SUPPORTED_STORES
            .iter()
            .map(|store| MemoryStoreInfo {
                store: (*store).to_string(),
                record_count: self
                    .memory_assertions
                    .iter()
                    .filter(|assertion| assertion.store == *store)
                    .count()
                    + self
                        .memory_conflicts
                        .values()
                        .filter(|signal| signal.store == *store)
                        .count(),
            })
            .collect()
    }

    fn persist_memory_state(&self) -> Result<(), MemoryIndexError> {
        let Some(backend) = self.memory_backend.as_ref() else {
            return Ok(());
        };
        let state = PersistedMemoryState {
            assertions: self.memory_assertions.clone(),
            conflicts: self.memory_conflicts.values().cloned().collect::<Vec<_>>(),
        };
        backend
            .write_state(&state)
            .map_err(MemoryIndexError::BackendPersistFailed)
    }

    pub fn memory_index(
        &mut self,
        request: MemoryIndexRequest,
    ) -> Result<MemoryIndexResult, MemoryIndexError> {
        let entity = Self::canonical_memory_token(&request.entity)
            .ok_or(MemoryIndexError::EntityRequired)?;
        let topic =
            Self::canonical_memory_token(&request.topic).ok_or(MemoryIndexError::TopicRequired)?;
        let store = match request.store.as_deref() {
            None => Self::default_memory_store(),
            Some(raw_store) => Self::canonical_memory_store(raw_store)
                .ok_or_else(|| MemoryIndexError::UnsupportedStore(raw_store.trim().to_string()))?,
        };
        let fact = request
            .fact
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let conflict = request.conflict.unwrap_or(false);
        if fact.is_none() && !conflict {
            return Err(MemoryIndexError::FactRequiredWhenConflictUnset);
        }

        let previous_memory_assertions = self.memory_assertions.clone();
        let previous_memory_conflicts = self.memory_conflicts.clone();
        let previous_memory_sequence = self.memory_sequence;

        let mut assertion_id = None;
        if let Some(fact) = fact {
            let assertion_sequence = self.next_memory_sequence();
            let assertion = MemoryAssertion {
                assertion_id: format!("memory-assert-{assertion_sequence:06}"),
                entity: entity.clone(),
                topic: topic.clone(),
                store: store.clone(),
                fact,
                metadata: request.metadata.clone(),
                indexed_at_ms: current_time_ms(),
            };
            assertion_id = Some(assertion.assertion_id.clone());
            self.memory_assertions.push(assertion);
            while self.memory_assertions.len() > MEMORY_ASSERTIONS_MAX_RETAINED {
                self.memory_assertions.remove(0);
            }
        }

        if conflict {
            let conflict_key = MemoryConflictKey {
                entity: entity.clone(),
                topic: topic.clone(),
                store: store.clone(),
            };
            self.memory_conflicts.insert(
                conflict_key,
                MemoryConflictSignal {
                    entity: entity.clone(),
                    topic: topic.clone(),
                    store: store.clone(),
                    reason: request
                        .conflict_reason
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string),
                    updated_at_ms: current_time_ms(),
                },
            );
        }
        if let Err(error) = self.persist_memory_state() {
            self.memory_assertions = previous_memory_assertions;
            self.memory_conflicts = previous_memory_conflicts;
            self.memory_sequence = previous_memory_sequence;
            return Err(error);
        }

        let conflict_active = self
            .memory_conflict_for_reference(Some(entity.as_str()), Some(topic.as_str()))
            .is_some();

        Ok(MemoryIndexResult {
            entity,
            topic,
            store,
            assertion_id,
            conflict_active,
        })
    }
    pub fn memory_query(&self, request: MemoryQueryRequest) -> MemoryQueryResult {
        let entity = request
            .entity
            .as_deref()
            .and_then(Self::canonical_memory_token);
        let topic = request
            .topic
            .as_deref()
            .and_then(Self::canonical_memory_token);
        let store = request
            .store
            .as_deref()
            .and_then(Self::canonical_memory_store);
        let assertions = self
            .memory_assertions
            .iter()
            .filter(|assertion| {
                entity
                    .as_ref()
                    .is_none_or(|value| assertion.entity.as_str() == value.as_str())
            })
            .filter(|assertion| {
                topic
                    .as_ref()
                    .is_none_or(|value| assertion.topic.as_str() == value.as_str())
            })
            .filter(|assertion| {
                store
                    .as_ref()
                    .is_none_or(|value| assertion.store.as_str() == value.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        let conflicts = self
            .memory_conflicts
            .values()
            .filter(|signal| {
                entity
                    .as_ref()
                    .is_none_or(|value| signal.entity.as_str() == value.as_str())
            })
            .filter(|signal| {
                topic
                    .as_ref()
                    .is_none_or(|value| signal.topic.as_str() == value.as_str())
            })
            .filter(|signal| {
                store
                    .as_ref()
                    .is_none_or(|value| signal.store.as_str() == value.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        MemoryQueryResult {
            assertions,
            conflicts,
        }
    }
}
