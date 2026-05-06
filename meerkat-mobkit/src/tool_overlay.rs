use std::collections::BTreeMap;

const IMAGE_GENERATION_BRIDGE_LIMIT: &str = "profiles.<name>.tools.image_generation cannot be profile-selective until a Meerkat release adds per-profile image_generation ToolConfig support";

use meerkat_mob::MobDefinition;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MobKitToolOverlayConfig {
    pub image_generation: bool,
}

#[derive(Debug, Clone, Default)]
struct MobKitToolOverlay {
    image_generation_by_profile: BTreeMap<String, bool>,
}

fn parse_mobkit_tool_overlay(toml_content: &str) -> Result<MobKitToolOverlay, String> {
    let value = toml_content
        .parse::<toml::Value>()
        .map_err(|err| err.to_string())?;
    let mut overlay = MobKitToolOverlay::default();
    let Some(profiles) = value.get("profiles").and_then(toml::Value::as_table) else {
        return Ok(overlay);
    };
    for (profile_name, profile_value) in profiles {
        let Some(tools) = profile_value.get("tools").and_then(toml::Value::as_table) else {
            continue;
        };
        if let Some(raw) = tools.get("image_generation") {
            let enabled = raw.as_bool().ok_or_else(|| {
                format!("profiles.{profile_name}.tools.image_generation must be boolean")
            })?;
            overlay
                .image_generation_by_profile
                .insert(profile_name.clone(), enabled);
        }
    }
    Ok(overlay)
}

fn validate_mobkit_tool_overlay(
    definition: &MobDefinition,
    overlay: &MobKitToolOverlay,
) -> Result<MobKitToolOverlayConfig, String> {
    let mut config = MobKitToolOverlayConfig::default();
    for (profile_name, enabled) in &overlay.image_generation_by_profile {
        if !*enabled {
            continue;
        }
        let profile = definition
            .profiles
            .get(&meerkat_mob::ProfileName::from(profile_name.as_str()))
            .and_then(|binding| binding.as_inline())
            .ok_or_else(|| {
                format!(
                    "profiles.{profile_name}.tools.image_generation can only be used on inline profiles"
                )
            })?;
        if !profile.tools.builtins {
            return Err(format!(
                "profiles.{profile_name}.tools.image_generation requires profiles.{profile_name}.tools.builtins = true"
            ));
        }
        config.image_generation = true;
    }

    if config.image_generation {
        for (profile_name, binding) in &definition.profiles {
            let Some(profile) = binding.as_inline() else {
                continue;
            };
            if !profile.tools.builtins {
                continue;
            }
            if overlay
                .image_generation_by_profile
                .get(profile_name.as_str())
                != Some(&true)
            {
                return Err(format!(
                    "{}; Meerkat 0.6 exposes only a mob-wide image_generation bridge, so every inline profile with tools.builtins = true must explicitly set tools.image_generation = true",
                    IMAGE_GENERATION_BRIDGE_LIMIT.replace("<name>", profile_name.as_str())
                ));
            }
        }
    }

    Ok(config)
}

pub fn validate_mobkit_tool_overlay_from_toml(
    definition: &MobDefinition,
    toml_content: &str,
) -> Result<MobKitToolOverlayConfig, String> {
    let overlay = parse_mobkit_tool_overlay(toml_content)?;
    validate_mobkit_tool_overlay(definition, &overlay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobkit_image_generation_overlay_defaults_off() -> Result<(), Box<dyn std::error::Error>> {
        let toml = r#"
[mob]
id = "image-gen-default-off"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.worker.tools]
builtins = true
"#;
        let definition = meerkat_mob::MobDefinition::from_toml(toml)?;

        validate_mobkit_tool_overlay_from_toml(&definition, toml).map_err(std::io::Error::other)?;
        Ok(())
    }

    #[test]
    fn mobkit_image_generation_overlay_false_is_allowed() -> Result<(), Box<dyn std::error::Error>>
    {
        let toml = r#"
[mob]
id = "image-gen-false"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.worker.tools]
builtins = true
image_generation = false
"#;
        let definition = meerkat_mob::MobDefinition::from_toml(toml)?;

        validate_mobkit_tool_overlay_from_toml(&definition, toml).map_err(std::io::Error::other)?;
        Ok(())
    }

    #[test]
    fn mobkit_image_generation_overlay_true_enables_temporary_bridge()
    -> Result<(), Box<dyn std::error::Error>> {
        let toml = r#"
[mob]
id = "image-gen-opt-in"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.worker.tools]
builtins = true
image_generation = true
"#;
        let definition = meerkat_mob::MobDefinition::from_toml(toml)?;

        let config = validate_mobkit_tool_overlay_from_toml(&definition, toml)
            .map_err(std::io::Error::other)?;
        assert!(config.image_generation);
        Ok(())
    }

    #[test]
    fn mobkit_image_generation_overlay_requires_builtins() -> Result<(), Box<dyn std::error::Error>>
    {
        let toml = r#"
[mob]
id = "image-gen-no-builtins"

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.worker.tools]
image_generation = true
"#;
        let definition = meerkat_mob::MobDefinition::from_toml(toml)?;

        let err = match validate_mobkit_tool_overlay_from_toml(&definition, toml) {
            Ok(_) => return Err(std::io::Error::other("builtins required").into()),
            Err(err) => err,
        };
        assert!(
            err.contains("tools.builtins = true"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    #[test]
    fn mobkit_image_generation_overlay_allows_mixed_builtin_profiles_when_disabled()
    -> Result<(), Box<dyn std::error::Error>> {
        let toml = r#"
[mob]
id = "image-gen-mixed"

[profiles.artist]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.artist.tools]
builtins = true
image_generation = false

[profiles.reviewer]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.reviewer.tools]
builtins = true
"#;
        let definition = meerkat_mob::MobDefinition::from_toml(toml)?;

        validate_mobkit_tool_overlay_from_toml(&definition, toml).map_err(std::io::Error::other)?;
        Ok(())
    }

    #[test]
    fn mobkit_image_generation_overlay_rejects_unmarked_builtin_profiles()
    -> Result<(), Box<dyn std::error::Error>> {
        let toml = r#"
[mob]
id = "image-gen-mixed-builtin"

[profiles.artist]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.artist.tools]
builtins = true
image_generation = true

[profiles.reviewer]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.reviewer.tools]
builtins = true
"#;
        let definition = meerkat_mob::MobDefinition::from_toml(toml)?;

        let err = match validate_mobkit_tool_overlay_from_toml(&definition, toml) {
            Ok(_) => return Err(std::io::Error::other("mixed builtin profiles should fail").into()),
            Err(err) => err,
        };
        assert!(
            err.contains("mob-wide image_generation bridge"),
            "unexpected error: {err}"
        );
        Ok(())
    }

    #[test]
    fn mobkit_image_generation_overlay_rejects_non_boolean()
    -> Result<(), Box<dyn std::error::Error>> {
        let toml = r#"
[mob]
id = "image-gen-bad-type"

[profiles.worker]
model = "gpt-5.5"

[profiles.worker.tools]
image_generation = "yes"
"#;
        let definition = meerkat_mob::MobDefinition::from_toml(toml)?;

        let err = match validate_mobkit_tool_overlay_from_toml(&definition, toml) {
            Ok(_) => return Err(std::io::Error::other("boolean required").into()),
            Err(err) => err,
        };
        assert!(err.contains("must be boolean"), "unexpected error: {err}");
        Ok(())
    }
}
