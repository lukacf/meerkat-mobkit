//! Parameter parsing for memory RPC methods.

use super::*;
use crate::identity_first::{
    AgentIdentity, AgentMemoryRecallRequest, AgentMemorySelection, NewAgentMemory,
};

const MEMORY_SUPPORTED_STORES: [&str; 5] = [
    "knowledge_graph",
    "vector",
    "timeline",
    "todo",
    "top_of_mind",
];
const DEFAULT_AGENT_MEMORY_MAX_ENTRIES: usize = 8;
const MAX_AGENT_MEMORY_MAX_ENTRIES: usize = 64;
const MAX_AGENT_MEMORY_TITLE_BYTES: usize = 200;
const MAX_AGENT_MEMORY_BODY_BYTES: usize = 64 * 1024;
const MAX_AGENT_MEMORY_TAGS: usize = 32;
const MAX_AGENT_MEMORY_TAG_BYTES: usize = 64;

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
    IdentityRequired,
    RealmMustBeString,
    MemoryIdRequired,
    TitleRequired,
    BodyRequired,
    TagsMustBeArray,
    TagMustBeString,
    TitleTooLong,
    BodyTooLong,
    TooManyTags,
    TagTooLong,
    SelectionMustBeString,
    UnsupportedSelection(String),
    QueryTermsMustBeArray,
    QueryTermMustBeString,
    MaxEntriesMustBePositiveInteger,
    MaxEntriesOutOfRange,
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
            MemoryParamsError::ParamsMustBeObject => "params must be a JSON object".to_string(),
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
            MemoryParamsError::EntityMustBeString => "entity filter must be a string".to_string(),
            MemoryParamsError::TopicMustBeString => "topic filter must be a string".to_string(),
            MemoryParamsError::IdentityRequired => {
                "identity must be a valid non-empty string".to_string()
            }
            MemoryParamsError::RealmMustBeString => {
                "realm must be a non-empty string when provided".to_string()
            }
            MemoryParamsError::MemoryIdRequired => {
                "memory_id must be a non-empty string".to_string()
            }
            MemoryParamsError::TitleRequired => "title must be a non-empty string".to_string(),
            MemoryParamsError::BodyRequired => "body must be a non-empty string".to_string(),
            MemoryParamsError::TagsMustBeArray => "tags must be an array when provided".to_string(),
            MemoryParamsError::TagMustBeString => {
                "tags must contain only non-empty strings".to_string()
            }
            MemoryParamsError::TitleTooLong => "title must be at most 200 bytes".to_string(),
            MemoryParamsError::BodyTooLong => "body must be at most 65536 bytes".to_string(),
            MemoryParamsError::TooManyTags => "tags must contain at most 32 entries".to_string(),
            MemoryParamsError::TagTooLong => "tags must be at most 64 bytes".to_string(),
            MemoryParamsError::SelectionMustBeString => {
                "selection must be 'always' or 'contextual' when provided".to_string()
            }
            MemoryParamsError::UnsupportedSelection(selection) => {
                format!("selection must be 'always' or 'contextual' (got '{selection}')")
            }
            MemoryParamsError::QueryTermsMustBeArray => {
                "query_terms must be an array when provided".to_string()
            }
            MemoryParamsError::QueryTermMustBeString => {
                "query_terms must contain only non-empty strings".to_string()
            }
            MemoryParamsError::MaxEntriesMustBePositiveInteger => {
                "max_entries must be a positive integer when provided".to_string()
            }
            MemoryParamsError::MaxEntriesOutOfRange => {
                "max_entries must be between 1 and 64".to_string()
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentMemoryRememberRpcRequest {
    pub(super) identity: AgentIdentity,
    pub(super) realm: String,
    pub(super) memory: NewAgentMemory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentMemoryForgetRpcRequest {
    pub(super) identity: AgentIdentity,
    pub(super) realm: String,
    pub(super) memory_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AgentMemoryRecallRpcRequest {
    pub(super) request: AgentMemoryRecallRequest,
}

pub(super) fn parse_memory_stores_params(params: &Value) -> Result<(), MemoryParamsError> {
    if params.is_null() || params.is_object() {
        return Ok(());
    }
    Err(MemoryParamsError::ParamsMustBeObject)
}

pub(super) fn parse_agent_memory_remember_params(
    params: &Value,
) -> Result<AgentMemoryRememberRpcRequest, MemoryParamsError> {
    let object = params
        .as_object()
        .ok_or(MemoryParamsError::ParamsMustBeObject)?;
    let identity = parse_agent_memory_identity(object)?;
    let realm = parse_agent_memory_realm(object)?;
    let title = object
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::TitleRequired)?
        .to_string();
    if title.len() > MAX_AGENT_MEMORY_TITLE_BYTES {
        return Err(MemoryParamsError::TitleTooLong);
    }
    let body = object
        .get("body")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::BodyRequired)?
        .to_string();
    if body.len() > MAX_AGENT_MEMORY_BODY_BYTES {
        return Err(MemoryParamsError::BodyTooLong);
    }
    let tags = match object.get("tags") {
        None => Vec::new(),
        Some(value) => {
            let tags = value.as_array().ok_or(MemoryParamsError::TagsMustBeArray)?;
            if tags.len() > MAX_AGENT_MEMORY_TAGS {
                return Err(MemoryParamsError::TooManyTags);
            }
            tags.iter()
                .map(|tag| {
                    let tag = tag
                        .as_str()
                        .ok_or(MemoryParamsError::TagMustBeString)?
                        .trim();
                    if tag.is_empty() {
                        return Err(MemoryParamsError::TagMustBeString);
                    }
                    if tag.len() > MAX_AGENT_MEMORY_TAG_BYTES {
                        return Err(MemoryParamsError::TagTooLong);
                    }
                    Ok(tag.to_string())
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    Ok(AgentMemoryRememberRpcRequest {
        identity,
        realm,
        memory: NewAgentMemory { title, body, tags },
    })
}

pub(super) fn parse_agent_memory_forget_params(
    params: &Value,
) -> Result<AgentMemoryForgetRpcRequest, MemoryParamsError> {
    let object = params
        .as_object()
        .ok_or(MemoryParamsError::ParamsMustBeObject)?;
    let identity = parse_agent_memory_identity(object)?;
    let realm = parse_agent_memory_realm(object)?;
    let memory_id = object
        .get("memory_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::MemoryIdRequired)?
        .to_string();
    Ok(AgentMemoryForgetRpcRequest {
        identity,
        realm,
        memory_id,
    })
}

pub(super) fn parse_agent_memory_recall_params(
    params: &Value,
) -> Result<AgentMemoryRecallRpcRequest, MemoryParamsError> {
    let object = params
        .as_object()
        .ok_or(MemoryParamsError::ParamsMustBeObject)?;
    let identity = parse_agent_memory_identity(object)?;
    let realm = parse_agent_memory_realm(object)?;
    let selection = match object.get("selection") {
        None => AgentMemorySelection::Contextual,
        Some(value) => {
            let selection = value
                .as_str()
                .ok_or(MemoryParamsError::SelectionMustBeString)?
                .trim()
                .to_ascii_lowercase();
            match selection.as_str() {
                "always" => AgentMemorySelection::Always,
                "contextual" => AgentMemorySelection::Contextual,
                _ => return Err(MemoryParamsError::UnsupportedSelection(selection)),
            }
        }
    };
    let max_entries = match object.get("max_entries") {
        None => DEFAULT_AGENT_MEMORY_MAX_ENTRIES,
        Some(value) => {
            let entries = value
                .as_u64()
                .ok_or(MemoryParamsError::MaxEntriesMustBePositiveInteger)?;
            if entries == 0 {
                return Err(MemoryParamsError::MaxEntriesMustBePositiveInteger);
            }
            let entries =
                usize::try_from(entries).map_err(|_| MemoryParamsError::MaxEntriesOutOfRange)?;
            if entries > MAX_AGENT_MEMORY_MAX_ENTRIES {
                return Err(MemoryParamsError::MaxEntriesOutOfRange);
            }
            entries
        }
    };
    let query_terms = match object.get("query_terms") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or(MemoryParamsError::QueryTermsMustBeArray)?
            .iter()
            .map(|term| {
                let term = term
                    .as_str()
                    .ok_or(MemoryParamsError::QueryTermMustBeString)?
                    .trim();
                if term.is_empty() {
                    return Err(MemoryParamsError::QueryTermMustBeString);
                }
                Ok(term.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
    };

    Ok(AgentMemoryRecallRpcRequest {
        request: AgentMemoryRecallRequest {
            identity,
            realm,
            query_terms,
            selection,
            max_entries,
        },
    })
}

fn parse_agent_memory_identity(
    object: &serde_json::Map<String, Value>,
) -> Result<AgentIdentity, MemoryParamsError> {
    object
        .get("identity")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(MemoryParamsError::IdentityRequired)
        .and_then(|value| {
            AgentIdentity::parse(value).map_err(|_| MemoryParamsError::IdentityRequired)
        })
}

fn parse_agent_memory_realm(
    object: &serde_json::Map<String, Value>,
) -> Result<String, MemoryParamsError> {
    match object.get("realm") {
        None => Ok("default".to_string()),
        Some(value) => {
            let realm = value
                .as_str()
                .ok_or(MemoryParamsError::RealmMustBeString)?
                .trim();
            if realm.is_empty() {
                return Err(MemoryParamsError::RealmMustBeString);
            }
            Ok(realm.to_string())
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::error::Error;

    #[test]
    fn agent_memory_recall_defaults_to_contextual_selection() -> Result<(), Box<dyn Error>> {
        let parsed = parse_agent_memory_recall_params(&json!({
            "identity": "identity:luka",
            "query_terms": ["passport"]
        }))
        .map_err(|err| std::io::Error::other(err.message()))?;

        assert_eq!(parsed.request.selection, AgentMemorySelection::Contextual);
        assert_eq!(parsed.request.realm, "default");
        assert_eq!(parsed.request.query_terms, vec!["passport".to_string()]);
        Ok(())
    }

    #[test]
    fn agent_memory_recall_preserves_explicit_always_selection() -> Result<(), Box<dyn Error>> {
        let parsed = parse_agent_memory_recall_params(&json!({
            "identity": "identity:luka",
            "selection": "always"
        }))
        .map_err(|err| std::io::Error::other(err.message()))?;

        assert_eq!(parsed.request.selection, AgentMemorySelection::Always);
        Ok(())
    }

    #[test]
    fn agent_memory_forget_requires_memory_id() {
        let err = parse_agent_memory_forget_params(&json!({
            "identity": "identity:luka"
        }))
        .err();

        assert_eq!(err, Some(MemoryParamsError::MemoryIdRequired));
    }

    #[test]
    fn agent_memory_forget_parses_identity_realm_and_memory_id() -> Result<(), Box<dyn Error>> {
        let parsed = parse_agent_memory_forget_params(&json!({
            "identity": "identity:luka",
            "realm": "family",
            "memory_id": "mem-1"
        }))
        .map_err(|err| std::io::Error::other(err.message()))?;

        assert_eq!(parsed.identity.as_str(), "identity:luka");
        assert_eq!(parsed.realm, "family");
        assert_eq!(parsed.memory_id, "mem-1");
        Ok(())
    }
}
