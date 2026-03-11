//! Parameter parsing for memory RPC methods.

use super::*;

const MEMORY_SUPPORTED_STORES: [&str; 5] = [
    "knowledge_graph",
    "vector",
    "timeline",
    "todo",
    "top_of_mind",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MemoryParamsError {
    ParamsMustBeObject,
    EntityRequired,
    TopicRequired,
    StoreMustBeString,
    UnsupportedStore(String),
    FactMustBeString,
    MetadataMustBeJson,
    ConflictMustBeBoolean,
    ConflictReasonMustBeString,
    EntityMustBeString,
    TopicMustBeString,
    Index(MemoryIndexError),
}

impl MemoryParamsError {
    pub(super) fn backend_message(error: &ElephantMemoryStoreError) -> String {
        match error {
            ElephantMemoryStoreError::InvalidConfig(reason)
            | ElephantMemoryStoreError::Io(reason)
            | ElephantMemoryStoreError::Serialize(reason)
            | ElephantMemoryStoreError::InvalidStoreData(reason)
            | ElephantMemoryStoreError::ExternalCallFailed(reason) => reason.clone(),
        }
    }

    pub(super) fn message(&self) -> String {
        match self {
            MemoryParamsError::ParamsMustBeObject => {
                "params must be a JSON object".to_string()
            }
            MemoryParamsError::EntityRequired => "entity must be a non-empty string".to_string(),
            MemoryParamsError::TopicRequired => "topic must be a non-empty string".to_string(),
            MemoryParamsError::StoreMustBeString => {
                "store must be a non-empty string when provided".to_string()
            }
            MemoryParamsError::UnsupportedStore(store) => format!(
                "store '{store}' is unsupported (allowed: knowledge_graph, vector, timeline, todo, top_of_mind)"
            ),
            MemoryParamsError::FactMustBeString => {
                "fact must be a non-empty string when provided".to_string()
            }
            MemoryParamsError::MetadataMustBeJson => {
                "metadata must be a JSON object when provided".to_string()
            }
            MemoryParamsError::ConflictMustBeBoolean => {
                "conflict must be a boolean when provided".to_string()
            }
            MemoryParamsError::ConflictReasonMustBeString => {
                "conflict_reason must be a string when provided".to_string()
            }
            MemoryParamsError::EntityMustBeString => {
                "entity filter must be a string".to_string()
            }
            MemoryParamsError::TopicMustBeString => {
                "topic filter must be a string".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::EntityRequired) => {
                "entity must be a non-empty string".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::TopicRequired) => {
                "topic must be a non-empty string".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::UnsupportedStore(store)) => format!(
                "store '{store}' is unsupported (allowed: knowledge_graph, vector, timeline, todo, top_of_mind)"
            ),
            MemoryParamsError::Index(MemoryIndexError::FactRequiredWhenConflictUnset) => {
                "fact is required unless conflict=true".to_string()
            }
            MemoryParamsError::Index(MemoryIndexError::BackendPersistFailed(error)) => {
                format!(
                    "memory backend persistence failed: {}",
                    Self::backend_message(error)
                )
            }
        }
    }
}

pub(super) fn parse_memory_stores_params(params: &Value) -> Result<(), MemoryParamsError> {
    if params.is_null() || params.is_object() {
        return Ok(());
    }
    Err(MemoryParamsError::ParamsMustBeObject)
}

fn parse_memory_store_field(value: &Value) -> Result<String, MemoryParamsError> {
    let store = value.as_str().ok_or(MemoryParamsError::StoreMustBeString)?;
    let canonical = store.trim().to_ascii_lowercase();
    if canonical.is_empty() {
        return Err(MemoryParamsError::StoreMustBeString);
    }
    if MEMORY_SUPPORTED_STORES.contains(&canonical.as_str()) {
        Ok(canonical)
    } else {
        Err(MemoryParamsError::UnsupportedStore(canonical))
    }
}

pub(super) fn parse_memory_index_params(
    params: &Value,
) -> Result<MemoryIndexRequest, MemoryParamsError> {
    let object = params
        .as_object()
        .ok_or(MemoryParamsError::ParamsMustBeObject)?;
    let entity = object
        .get("entity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::EntityRequired)?;
    let topic = object
        .get("topic")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::TopicRequired)?;
    let store = match object.get("store") {
        None => None,
        Some(value) => Some(parse_memory_store_field(value)?),
    };
    let fact = match object.get("fact") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::FactMustBeString)?
                .trim()
                .to_string(),
        ),
    };
    if fact.as_deref().is_some_and(str::is_empty) {
        return Err(MemoryParamsError::FactMustBeString);
    }
    let metadata = match object.get("metadata") {
        None => None,
        Some(value) => {
            if !value.is_object() {
                return Err(MemoryParamsError::MetadataMustBeJson);
            }
            Some(value.clone())
        }
    };
    let conflict = match object.get("conflict") {
        None => None,
        Some(value) => Some(
            value
                .as_bool()
                .ok_or(MemoryParamsError::ConflictMustBeBoolean)?,
        ),
    };
    let conflict_reason = match object.get("conflict_reason") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::ConflictReasonMustBeString)?
                .to_string(),
        ),
    };

    Ok(MemoryIndexRequest {
        entity: entity.to_string(),
        topic: topic.to_string(),
        store,
        fact,
        metadata,
        conflict,
        conflict_reason,
    })
}

pub(super) fn parse_memory_query_params(
    params: &Value,
) -> Result<MemoryQueryRequest, MemoryParamsError> {
    if params.is_null() {
        return Ok(MemoryQueryRequest {
            entity: None,
            topic: None,
            store: None,
        });
    }
    let object = params
        .as_object()
        .ok_or(MemoryParamsError::ParamsMustBeObject)?;
    let entity = match object.get("entity") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::EntityMustBeString)?
                .to_string(),
        ),
    };
    let topic = match object.get("topic") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(MemoryParamsError::TopicMustBeString)?
                .to_string(),
        ),
    };
    let store = match object.get("store") {
        None => None,
        Some(value) => Some(parse_memory_store_field(value)?),
    };
    Ok(MemoryQueryRequest {
        entity,
        topic,
        store,
    })
}
