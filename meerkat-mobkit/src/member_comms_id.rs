//! Single owner of the mapping between MobKit's public member aliases and
//! the mob-roster member ids handed to meerkat-mob.
//!
//! Meerkat 0.7 made the mob member comms name a fail-closed typed owner
//! (`meerkat_core::connection::MemberCommsName`): every component — including
//! the member id — must start with an ASCII letter or `_` and contain only
//! ASCII alphanumerics, `-`, or `_`. MobKit's identity-first surface mints
//! runtime-id-shaped member identities (`rt:{identity}:{generation}`, where
//! the durable identity itself conventionally contains `:`, e.g.
//! `review:singleton`), which 0.6.34 accepted (the comms name was an
//! unvalidated `format!`) and 0.7 rejects at spawn.
//!
//! The public alias space is unchanged: consoles, RPC surfaces, SDKs, and
//! persisted continuity records keep speaking `rt:review:singleton:0`. This
//! module encodes those aliases into comms-safe roster ids at the
//! mobkit→meerkat-mob boundary and decodes roster ids back to public aliases
//! at projection boundaries.
//!
//! Encoding contract:
//! - An alias that is already a valid comms-name component (and does not
//!   start with the reserved `mk--` marker) maps to itself, so plain
//!   definition-mob member names are untouched.
//! - Anything else maps to `mk--` + escaped body, where `_` → `__`,
//!   `:` → `_c`, and any other non-`[A-Za-z0-9-]` char → `_x{hex}_`.
//! - `decode(encode(s)) == s` for every alias, and `decode` is the identity
//!   on ids that were never encoded. The `mk--` prefix is a reserved
//!   namespace: user-chosen member names must not start with it (encode
//!   re-encodes such names so the round-trip still holds).

use std::borrow::Cow;

/// Reserved marker prefix for encoded member ids.
const MARKER: &str = "mk--";

/// True when `s` is a valid meerkat 0.7 comms-name component
/// (mirrors `meerkat_core::connection::validate_member_comms_name_component`).
fn is_valid_comms_component(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn escape_body(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '_' => out.push_str("__"),
            ':' => out.push_str("_c"),
            c if c.is_ascii_alphanumeric() || c == '-' => out.push(c),
            c => {
                out.push_str("_x");
                out.push_str(&format!("{:x}", c as u32));
                out.push('_');
            }
        }
    }
    out
}

/// Decode an escaped body; `None` when the body is not a well-formed escape
/// production (such an id cannot have come from [`mob_member_id_str`]).
fn unescape_body(s: &str) -> Option<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '_' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '_' => out.push('_'),
            'c' => out.push(':'),
            'x' => {
                let mut hex = String::new();
                loop {
                    match chars.next()? {
                        '_' => break,
                        h => hex.push(h),
                    }
                }
                let code = u32::from_str_radix(&hex, 16).ok()?;
                out.push(char::from_u32(code)?);
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Map a public member alias (identity-first runtime id, durable identity, or
/// plain member name) to the comms-safe mob roster member id string.
pub(crate) fn mob_member_id_str(alias: &str) -> Cow<'_, str> {
    if is_valid_comms_component(alias) && !alias.starts_with(MARKER) {
        Cow::Borrowed(alias)
    } else {
        Cow::Owned(format!("{MARKER}{}", escape_body(alias)))
    }
}

/// Map a public member alias to a typed mob roster member id.
pub(crate) fn mob_member_id(alias: &str) -> meerkat_mob::ids::AgentIdentity {
    meerkat_mob::ids::AgentIdentity::from(mob_member_id_str(alias).as_ref())
}

/// Map a mob roster member id back to the public member alias. Identity on
/// ids that were never encoded.
pub(crate) fn runtime_alias_str(member_id: &str) -> Cow<'_, str> {
    match member_id.strip_prefix(MARKER) {
        Some(body) => match unescape_body(body) {
            Some(alias) => Cow::Owned(alias),
            // Not an encode production; treat as a literal member id.
            None => Cow::Borrowed(member_id),
        },
        None => Cow::Borrowed(member_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_member_names_pass_through_unchanged() {
        for name in ["worker", "worker-one", "_internal", "Agent7"] {
            assert_eq!(mob_member_id_str(name), name);
            assert_eq!(runtime_alias_str(name), name);
        }
    }

    #[test]
    fn colon_aliases_round_trip_and_are_comms_safe() {
        for alias in [
            "rt:review:singleton:0",
            "rt:channel:C0SMOKEOB3:0",
            "agent:beta",
            "review:singleton",
            "rt:agent-x:0",
            "with_underscore:and:colons",
        ] {
            let encoded = mob_member_id_str(alias);
            assert!(
                is_valid_comms_component(&encoded),
                "{encoded:?} must satisfy meerkat 0.7 MemberCommsName"
            );
            assert_eq!(runtime_alias_str(&encoded), alias, "round trip of {alias}");
        }
    }

    #[test]
    fn non_ascii_and_punctuation_round_trip() {
        for alias in ["user@host", "a.b:c", "9starts-with-digit", "ünïcode:1"] {
            let encoded = mob_member_id_str(alias);
            assert!(is_valid_comms_component(&encoded), "{encoded:?}");
            assert_eq!(runtime_alias_str(&encoded), alias);
        }
    }

    #[test]
    fn reserved_marker_names_re_encode_so_round_trip_holds() {
        let alias = "mk--rt_creview";
        let encoded = mob_member_id_str(alias);
        assert_ne!(encoded, alias, "marker-prefixed names must be re-encoded");
        assert_eq!(runtime_alias_str(&encoded), alias);
    }

    #[test]
    fn encoding_is_injective_across_pass_through_and_encoded_forms() {
        let inputs = [
            "rt:review:singleton:0",
            "rt-review-singleton-0",
            "rt_creview_csingleton_c0",
            "mk--rt_creview_csingleton_c0",
        ];
        let mut seen = std::collections::BTreeSet::new();
        for input in inputs {
            assert!(
                seen.insert(mob_member_id_str(input).into_owned()),
                "collision on {input}"
            );
        }
    }
}
