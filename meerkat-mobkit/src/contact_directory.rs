//! Contact directory — maps mob IDs to transport info for cross-mob communication.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Transport for reaching an external mob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MobTransport {
    /// Same process — use InprocRegistry namespace lookup.
    Inproc,
    /// Remote process — TCP connection.
    Tcp(String),
    /// Remote process — Unix domain socket.
    Uds(String),
}

/// Entry in the contact directory for one external mob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactEntry {
    pub mob_id: String,
    pub transport: MobTransport,
}

/// Error loading or parsing a contact directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContactDirectoryError {
    /// TOML parsing failed.
    Parse(String),
    /// Invalid transport string.
    InvalidTransport { mob_id: String, value: String },
}

impl std::fmt::Display for ContactDirectoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(reason) => write!(f, "contact directory parse error: {reason}"),
            Self::InvalidTransport { mob_id, value } => {
                write!(f, "invalid transport for mob '{mob_id}': {value}")
            }
        }
    }
}

impl std::error::Error for ContactDirectoryError {}

/// The contact directory — maps mob IDs to connection info.
///
/// Loaded from TOML config at startup. Immutable after construction.
#[derive(Debug, Clone, Default)]
pub struct ContactDirectory {
    entries: BTreeMap<String, ContactEntry>,
}

impl ContactDirectory {
    /// Parse a contact directory from TOML.
    ///
    /// Expected format:
    /// ```toml
    /// [mobs]
    /// google-workspace = "inproc"
    /// home-assistant = "tcp://192.168.1.50:9002"
    /// smart-home = "uds:///var/run/meerkat/smart-home.sock"
    /// ```
    pub fn from_toml(text: &str) -> Result<Self, ContactDirectoryError> {
        let table: toml::Value =
            toml::from_str(text).map_err(|e| ContactDirectoryError::Parse(e.to_string()))?;

        let mobs = table
            .get("mobs")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();

        let mut entries = BTreeMap::new();
        for (mob_id, value) in mobs {
            let transport_str =
                value
                    .as_str()
                    .ok_or_else(|| ContactDirectoryError::InvalidTransport {
                        mob_id: mob_id.clone(),
                        value: format!("{value}"),
                    })?;
            let transport = parse_transport(transport_str).ok_or_else(|| {
                ContactDirectoryError::InvalidTransport {
                    mob_id: mob_id.clone(),
                    value: transport_str.to_string(),
                }
            })?;
            entries.insert(mob_id.clone(), ContactEntry { mob_id, transport });
        }

        Ok(Self { entries })
    }

    /// Look up a mob by ID.
    pub fn get(&self, mob_id: &str) -> Option<&ContactEntry> {
        self.entries.get(mob_id)
    }

    /// Check if a mob ID is in the directory.
    pub fn contains(&self, mob_id: &str) -> bool {
        self.entries.contains_key(mob_id)
    }

    /// List all entries.
    pub fn list(&self) -> Vec<&ContactEntry> {
        self.entries.values().collect()
    }
}

fn parse_transport(s: &str) -> Option<MobTransport> {
    match s {
        "inproc" => Some(MobTransport::Inproc),
        _ if s.starts_with("tcp://") => Some(MobTransport::Tcp(s[6..].to_string())),
        _ if s.starts_with("uds://") => Some(MobTransport::Uds(s[6..].to_string())),
        _ => None,
    }
}

/// Parse `"member::mob_id"` into `(member, mob_id)`.
///
/// Uses `::` as separator (not `@`) to avoid collision with email-based
/// member IDs like `personal:luka@king.com`.
///
/// Only matches if `mob_id` is a known entry in the directory —
/// bare member names and unknown mob IDs return `None`.
pub fn parse_cross_mob_address<'a>(
    address: &'a str,
    directory: &ContactDirectory,
) -> Option<(&'a str, &'a str)> {
    let sep = address.rfind("::")?;
    let member = &address[..sep];
    let mob_id = &address[sep + 2..];
    if member.is_empty() || mob_id.is_empty() {
        return None;
    }
    if !directory.contains(mob_id) {
        return None;
    }
    Some((member, mob_id))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_toml() {
        let dir = ContactDirectory::from_toml(
            r#"
            [mobs]
            google-workspace = "inproc"
            home-assistant = "tcp://192.168.1.50:9002"
            smart-home = "uds:///var/run/meerkat/smart-home.sock"
            "#,
        )
        .unwrap();
        assert_eq!(dir.list().len(), 3);
        assert_eq!(
            dir.get("google-workspace").unwrap().transport,
            MobTransport::Inproc
        );
        assert_eq!(
            dir.get("home-assistant").unwrap().transport,
            MobTransport::Tcp("192.168.1.50:9002".to_string())
        );
        assert_eq!(
            dir.get("smart-home").unwrap().transport,
            MobTransport::Uds("/var/run/meerkat/smart-home.sock".to_string())
        );
    }

    #[test]
    fn parse_empty_toml() {
        let dir = ContactDirectory::from_toml("[mobs]").unwrap();
        assert!(dir.list().is_empty());
    }

    #[test]
    fn parse_missing_mobs_section() {
        let dir = ContactDirectory::from_toml("").unwrap();
        assert!(dir.list().is_empty());
    }

    #[test]
    fn parse_invalid_transport() {
        let result = ContactDirectory::from_toml(
            r#"
            [mobs]
            bad = "ftp://nope"
            "#,
        );
        assert!(matches!(
            result,
            Err(ContactDirectoryError::InvalidTransport { .. })
        ));
    }

    #[test]
    fn cross_mob_address_parsing() {
        let dir = ContactDirectory::from_toml(
            r#"
            [mobs]
            google-workspace = "inproc"
            "#,
        )
        .unwrap();

        // Valid cross-mob address
        assert_eq!(
            parse_cross_mob_address("calendar::google-workspace", &dir),
            Some(("calendar", "google-workspace"))
        );

        // Bare member name — no match
        assert_eq!(parse_cross_mob_address("calendar", &dir), None);

        // Unknown mob — no match
        assert_eq!(parse_cross_mob_address("calendar::unknown-mob", &dir), None);

        // Email-based member ID — no match (no :: separator)
        assert_eq!(
            parse_cross_mob_address("personal:luka@king.com", &dir),
            None
        );

        // Empty parts
        assert_eq!(parse_cross_mob_address("::google-workspace", &dir), None);
        assert_eq!(parse_cross_mob_address("calendar::", &dir), None);
    }
}
