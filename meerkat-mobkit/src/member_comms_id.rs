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
//!
//! This is a **public stability surface**: consumers that talk to raw
//! `MobHandle` APIs (which speak the encoded roster-id space on meerkat 0.7+)
//! must encode aliases on the way in and decode roster ids on the way out
//! using exactly this codec — there is no separate "correct" encoding. Use
//! [`mob_member_id`]/[`mob_member_id_str`] at the mobkit→meerkat-mob boundary
//! and [`runtime_alias_str`]/[`runtime_event_alias`] at projection boundaries.

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
pub fn mob_member_id_str(alias: &str) -> Cow<'_, str> {
    if is_valid_comms_component(alias) && !alias.starts_with(MARKER) {
        Cow::Borrowed(alias)
    } else {
        Cow::Owned(format!("{MARKER}{}", escape_body(alias)))
    }
}

/// Map a public member alias to a typed mob roster member id.
pub fn mob_member_id(alias: &str) -> meerkat_mob::ids::AgentIdentity {
    meerkat_mob::ids::AgentIdentity::from(mob_member_id_str(alias).as_ref())
}

/// Map a mob roster member id back to the public member alias. Identity on
/// ids that were never encoded.
pub fn runtime_alias_str(member_id: &str) -> Cow<'_, str> {
    match member_id.strip_prefix(MARKER) {
        Some(body) => match unescape_body(body) {
            Some(alias) => Cow::Owned(alias),
            // Not an encode production; treat as a literal member id.
            None => Cow::Borrowed(member_id),
        },
        None => Cow::Borrowed(member_id),
    }
}

/// Project a mob runtime id (`{roster_member_id}:{generation}`) into the
/// public alias space (`{alias}:{generation}`).
///
/// Agent events leave meerkat-mob keyed by [`AgentRuntimeId`]s built from
/// roster binding atoms; the member-id component is the comms-safe encoding,
/// so it must be decoded before any console/SDK projection — console replay
/// resolution, the `mobkit/events/subscribe` buffer, `/mob/events` SSE, and
/// per-agent ABAC view checks all speak the alias space.
pub fn runtime_event_alias(runtime_id: &meerkat_mob::ids::AgentRuntimeId) -> String {
    format!(
        "{}:{}",
        runtime_alias_str(runtime_id.identity.as_str()),
        runtime_id.generation.get()
    )
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
    fn runtime_event_alias_decodes_encoded_roster_member_ids() {
        use meerkat_mob::ids::{AgentIdentity, AgentRuntimeId, Generation};

        // Identity-first alias: the roster id is the comms-safe encoding and
        // must decode back to the public alias before any event projection.
        let encoded = mob_member_id_str("rt:review:singleton:0").into_owned();
        let runtime_id =
            AgentRuntimeId::new(AgentIdentity::from(encoded.as_str()), Generation::new(1));
        assert_eq!(runtime_event_alias(&runtime_id), "rt:review:singleton:0:1");

        // Plain member names pass through unchanged.
        let runtime_id = AgentRuntimeId::initial(AgentIdentity::from("worker-one"));
        assert_eq!(runtime_event_alias(&runtime_id), "worker-one:0");
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

    // --- Defensive suite: map the whole id codec + reconciliation aliases ---

    /// The exact identities + runtime-id forms from the HomeCore deployment
    /// that surfaced the meerkat 0.7 comms-name regression, plus the escape
    /// productions. Every entry must encode to a comms-safe id and round-trip.
    fn defensive_corpus() -> Vec<String> {
        let mut v: Vec<String> = [
            // Plain definition-mob names (pass through unchanged).
            "worker",
            "worker-one",
            "_internal",
            "Agent7",
            "a",
            // HomeCore durable identities (colon-bearing).
            "identity:parent-1",
            "identity:parent-2",
            "identity:child-1",
            "identity:child-2",
            "domain:calendar",
            "domain:school",
            "domain:health",
            "domain:home",
            "domain:home-automation",
            "domain:finance",
            "domain:discovery",
            "family-group:main",
            "triage:main",
            "gate:main",
            // Runtime-id shaped aliases (`rt:{identity}:{generation}`).
            "rt:identity:parent-1:0",
            "rt:domain:home-automation:3",
            "rt:channel:C0SMOKEOB3:0",
            "rt:review:singleton:12",
            // Mixed punctuation / boundary shapes.
            "a:b_c",
            "with_underscore:and:colons",
            "user@host",
            "a.b:c",
            "9starts-with-digit",
            "::",
            ":",
            "-",
            "mk--collision",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        // Non-ASCII, multi-byte, and control characters.
        v.push("ünïcode:1".to_string());
        v.push("emoji-😀:2".to_string());
        v.push("Ω≈ç:3".to_string());
        v.push("tab\tnl\n".to_string());
        v.push("\u{0}\u{1f}x".to_string());
        v.push("a b".to_string());
        v.push(String::new());
        v
    }

    #[test]
    fn homecore_corpus_round_trips_and_is_comms_safe() {
        for alias in defensive_corpus() {
            let encoded = mob_member_id_str(&alias).into_owned();
            assert!(
                is_valid_comms_component(&encoded),
                "encoded {encoded:?} (from {alias:?}) is not a valid comms component"
            );
            assert!(
                !encoded.contains([':', '/']),
                "encoded {encoded:?} leaks a routing separator"
            );
            assert_eq!(
                runtime_alias_str(&encoded),
                alias,
                "round trip of {alias:?}"
            );
        }
    }

    /// The load-bearing guard: the codec's output must be accepted by meerkat
    /// 0.7's own fail-closed comms-name validator — the exact check that
    /// rejected `rt:identity:parent-1:0` before this codec existed. If meerkat
    /// tightens `MemberCommsName` again, this test fails instead of HomeCore.
    #[test]
    fn encoded_ids_satisfy_meerkat_member_comms_name_validator() {
        use meerkat_core::connection::MemberCommsName;
        for alias in defensive_corpus() {
            let encoded = mob_member_id_str(&alias).into_owned();
            assert!(
                MemberCommsName::new("homecore-mob", "worker", encoded.clone()).is_ok(),
                "MemberCommsName::new rejected encoded id {encoded:?} (from alias {alias:?})"
            );
        }
    }

    /// Pin the on-the-wire encoding so an accidental codec change is caught.
    #[test]
    fn exact_wire_format_for_known_aliases() {
        let cases = [
            ("worker-one", "worker-one"), // already comms-safe -> pass through
            ("identity:parent-1", "mk--identity_cparent-1"),
            ("domain:home-automation", "mk--domain_chome-automation"),
            ("family-group:main", "mk--family-group_cmain"),
            ("rt:identity:parent-1:0", "mk--rt_cidentity_cparent-1_c0"),
            ("triage:main", "mk--triage_cmain"),
            ("a:b_c", "mk--a_cb__c"),
            ("a b", "mk--a_x20_b"),
            ("", "mk--"),
        ];
        for (alias, expected) in cases {
            assert_eq!(mob_member_id_str(alias), expected, "encode {alias:?}");
            assert_eq!(runtime_alias_str(expected), alias, "decode {expected:?}");
        }
    }

    /// Ids that begin with the marker but are not well-formed escape productions
    /// must decode to themselves (literal fallback) and never panic.
    #[test]
    fn malformed_encoded_bodies_decode_to_literal_without_panic() {
        for bad in [
            "mk--_",           // dangling underscore (no escape selector)
            "mk--_z",          // invalid escape selector
            "mk--_x",          // truncated hex escape (no terminator)
            "mk--_x_",         // empty hex body
            "mk--_xZZ_",       // non-hex digits
            "mk--_x110000_",   // code point above the Unicode max
            "mk--_xffffffff_", // parses as u32 but is not a valid char
            "mk--abc_",        // trailing dangling underscore after a valid run
        ] {
            assert_eq!(runtime_alias_str(bad), bad, "literal fallback for {bad:?}");
        }
    }

    /// Empty, single-character, and marker-adjacent inputs all round-trip.
    #[test]
    fn empty_and_boundary_strings_round_trip() {
        // Empty alias is not a valid comms component, so it becomes the bare
        // marker and decodes back to empty.
        assert_eq!(mob_member_id_str(""), "mk--");
        assert_eq!(runtime_alias_str("mk--"), "");
        for alias in [":", "_", "-", "::", "_x", "mk", "mk-", "mk--"] {
            let encoded = mob_member_id_str(alias).into_owned();
            assert!(
                is_valid_comms_component(&encoded),
                "{encoded:?} not comms-safe"
            );
            assert_eq!(runtime_alias_str(&encoded), alias, "round trip {alias:?}");
        }
    }

    /// Canonicalization contract used by the schedule internal-delivery lane:
    /// decode-then-encode maps BOTH the alias space and the roster-id space to
    /// the same roster key, and is the identity on plain member names. Encode
    /// alone is NOT idempotent on roster ids (the reserved-marker rule
    /// re-encodes them) — the 0.7.28 HomeCore schedule self-delivery failure.
    #[test]
    fn decode_then_encode_canonicalizes_both_id_spaces() {
        for (input, canonical) in [
            ("rt:domain:home:0", "mk--rt_cdomain_chome_c0"),
            ("mk--rt_cdomain_chome_c0", "mk--rt_cdomain_chome_c0"),
            ("domain:home", "mk--domain_chome"),
            ("mk--domain_chome", "mk--domain_chome"),
            ("digest-owner", "digest-owner"),
        ] {
            let key = mob_member_id_str(runtime_alias_str(input).as_ref()).into_owned();
            assert_eq!(key, canonical, "canonical roster key for {input:?}");
        }
        // The failure mode this contract exists to prevent:
        assert_ne!(
            mob_member_id_str("mk--rt_cdomain_chome_c0"),
            "mk--rt_cdomain_chome_c0",
            "encode alone re-encodes roster ids — never use it on binding member ids"
        );
    }

    /// Decode only applies inside the reserved marker namespace; a raw id that
    /// merely *looks* like an escape body must pass through untouched.
    #[test]
    fn decode_is_identity_on_unencoded_ids() {
        for id in [
            "worker",
            "worker-one",
            "_internal",
            "Agent7",
            "rt-review-singleton-0",
            "a_cb", // not marker-prefixed -> NOT decoded to "a:b"
        ] {
            assert_eq!(runtime_alias_str(id), id);
        }
    }

    #[test]
    fn runtime_event_alias_across_generations_and_forms() {
        use meerkat_mob::ids::{AgentIdentity, AgentRuntimeId, Generation};

        let encoded = mob_member_id_str("rt:identity:parent-1:0").into_owned();
        let rid = AgentRuntimeId::new(AgentIdentity::from(encoded.as_str()), Generation::new(7));
        assert_eq!(runtime_event_alias(&rid), "rt:identity:parent-1:0:7");

        let encoded = mob_member_id_str("domain:home-automation").into_owned();
        let rid = AgentRuntimeId::new(AgentIdentity::from(encoded.as_str()), Generation::new(1));
        assert_eq!(runtime_event_alias(&rid), "domain:home-automation:1");

        // Plain member name keeps its initial generation.
        let rid = AgentRuntimeId::initial(AgentIdentity::from("triage-main"));
        assert_eq!(runtime_event_alias(&rid), "triage-main:0");
    }

    /// Exhaustive total round-trip + comms-safety + injectivity over every
    /// string of length 0..=3 from a charset that exercises every escape branch.
    #[test]
    fn round_trip_is_total_and_injective_over_generated_corpus() {
        use meerkat_core::connection::MemberCommsName;
        use std::collections::BTreeMap;

        let charset = ['a', 'Z', '0', '-', '_', ':', '.', '@', ' ', 'ü', '😀'];
        let mut aliases: Vec<String> = vec![String::new()];
        let mut frontier = vec![String::new()];
        for _ in 0..3 {
            let mut next = Vec::new();
            for prefix in &frontier {
                for c in charset {
                    let mut s = prefix.clone();
                    s.push(c);
                    aliases.push(s.clone());
                    next.push(s);
                }
            }
            frontier = next;
        }

        let mut encoded_to_alias: BTreeMap<String, String> = BTreeMap::new();
        for alias in &aliases {
            let encoded = mob_member_id_str(alias).into_owned();
            assert!(
                MemberCommsName::new("m", "r", encoded.clone()).is_ok(),
                "encoded {encoded:?} (from {alias:?}) is not comms-safe"
            );
            assert_eq!(
                &runtime_alias_str(&encoded).into_owned(),
                alias,
                "round trip of {alias:?}"
            );
            if let Some(prev) = encoded_to_alias.insert(encoded.clone(), alias.clone()) {
                assert_eq!(
                    &prev, alias,
                    "collision: {prev:?} and {alias:?} both encode to {encoded:?}"
                );
            }
        }
    }
}
