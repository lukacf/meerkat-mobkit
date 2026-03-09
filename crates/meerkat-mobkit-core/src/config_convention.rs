//! Convention-based config discovery for MobKit applications.
//!
//! Applications follow a directory convention:
//!
//! ```text
//! config/
//!   mob.toml                    # mob definition (profiles, wiring, skills)
//!   gating.toml                 # gating rules (optional)
//!   defaults/
//!     schedules.toml            # default schedule definitions (optional)
//! deployment/
//!   routing.toml                # deployment-specific routing (optional)
//!   schedules.toml              # deployment-specific schedules (optional)
//! ```
//!
//! If a file exists at the conventional path, it's loaded. If not, it's skipped.
//! Explicit paths always override convention.
//!
//! # Usage
//!
//! ```rust,no_run
//! use meerkat_mobkit_core::ConventionalPaths;
//!
//! let paths = ConventionalPaths::discover("config", "deployment");
//! println!("mob: {:?}", paths.mob_toml);
//! println!("gating: {:?}", paths.gating_toml);
//! println!("schedule files: {:?}", paths.schedule_files);
//! ```

use std::path::{Path, PathBuf};

/// Discovered config file paths from conventional directory layout.
///
/// All paths are relative to the working directory. Fields are `Option` —
/// `None` means the file was not found at the conventional location.
#[derive(Debug, Clone, Default)]
pub struct ConventionalPaths {
    /// Mob definition TOML (e.g. `config/mob.toml`).
    pub mob_toml: Option<PathBuf>,
    /// Gating config (e.g. `config/gating.toml`).
    pub gating_toml: Option<PathBuf>,
    /// Routing config (e.g. `deployment/routing.toml`).
    pub routing_toml: Option<PathBuf>,
    /// All discovered schedule files, in order:
    /// defaults first (e.g. `config/defaults/schedules.toml`),
    /// then deployment overrides (e.g. `deployment/schedules.toml`).
    pub schedule_files: Vec<PathBuf>,
}

impl ConventionalPaths {
    /// Discover config files from conventional directory layout.
    ///
    /// Checks fixed paths relative to the working directory.
    /// Only includes files that actually exist on disk.
    pub fn discover(config_dir: impl AsRef<Path>, deployment_dir: impl AsRef<Path>) -> Self {
        let config = config_dir.as_ref();
        let deployment = deployment_dir.as_ref();

        let mob_toml = check_file(config.join("mob.toml"));
        let gating_toml = check_file(config.join("gating.toml"));
        let routing_toml = check_file(deployment.join("routing.toml"));

        let mut schedule_files = Vec::new();
        if let Some(p) = check_file(config.join("defaults").join("schedules.toml")) {
            schedule_files.push(p);
        }
        if let Some(p) = check_file(deployment.join("schedules.toml")) {
            schedule_files.push(p);
        }

        let result = Self {
            mob_toml,
            gating_toml,
            routing_toml,
            schedule_files,
        };

        // Log what was discovered
        if let Some(ref p) = result.mob_toml {
            eprintln!("config: mob definition at {}", p.display());
        }
        if let Some(ref p) = result.gating_toml {
            eprintln!("config: gating rules at {}", p.display());
        }
        if let Some(ref p) = result.routing_toml {
            eprintln!("config: routing at {}", p.display());
        }
        for p in &result.schedule_files {
            eprintln!("config: schedule file at {}", p.display());
        }

        result
    }

    /// Read and parse the mob definition TOML.
    ///
    /// Returns `None` if `mob_toml` is not set.
    pub fn read_mob_definition(&self) -> Result<Option<meerkat_mob::MobDefinition>, String> {
        let path = match &self.mob_toml {
            Some(p) => p,
            None => return Ok(None),
        };
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let definition = meerkat_mob::MobDefinition::from_toml(&content)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
        Ok(Some(definition))
    }

    /// Read a TOML file as a string, if the path is set.
    pub fn read_toml(&self, path: &Option<PathBuf>) -> Result<Option<String>, String> {
        let path = match path {
            Some(p) => p,
            None => return Ok(None),
        };
        std::fs::read_to_string(path)
            .map(Some)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))
    }

    /// Collect schedule file paths as strings (for module args).
    pub fn schedule_file_strings(&self) -> Vec<String> {
        self.schedule_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    }
}

fn check_file(path: PathBuf) -> Option<PathBuf> {
    if path.is_file() {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discover_finds_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let deployment = tmp.path().join("deployment");
        fs::create_dir_all(config.join("defaults")).unwrap();
        fs::create_dir_all(&deployment).unwrap();

        fs::write(config.join("mob.toml"), "[mob]\nid = \"test\"").unwrap();
        fs::write(config.join("gating.toml"), "[[rules]]").unwrap();
        fs::write(
            config.join("defaults").join("schedules.toml"),
            "[[schedules]]",
        )
        .unwrap();
        fs::write(deployment.join("routing.toml"), "[[routes]]").unwrap();
        fs::write(deployment.join("schedules.toml"), "[[schedules]]").unwrap();

        let paths = ConventionalPaths::discover(&config, &deployment);
        assert!(paths.mob_toml.is_some());
        assert!(paths.gating_toml.is_some());
        assert!(paths.routing_toml.is_some());
        assert_eq!(paths.schedule_files.len(), 2);
    }

    #[test]
    fn discover_handles_missing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let deployment = tmp.path().join("deployment");
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&deployment).unwrap();

        // Only mob.toml exists
        fs::write(config.join("mob.toml"), "[mob]\nid = \"test\"").unwrap();

        let paths = ConventionalPaths::discover(&config, &deployment);
        assert!(paths.mob_toml.is_some());
        assert!(paths.gating_toml.is_none());
        assert!(paths.routing_toml.is_none());
        assert!(paths.schedule_files.is_empty());
    }

    #[test]
    fn discover_handles_nonexistent_dirs() {
        let paths = ConventionalPaths::discover("/nonexistent/config", "/nonexistent/deployment");
        assert!(paths.mob_toml.is_none());
        assert!(paths.gating_toml.is_none());
        assert!(paths.routing_toml.is_none());
        assert!(paths.schedule_files.is_empty());
    }

    #[test]
    fn schedule_files_ordered_defaults_first() {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let deployment = tmp.path().join("deployment");
        fs::create_dir_all(config.join("defaults")).unwrap();
        fs::create_dir_all(&deployment).unwrap();

        fs::write(
            config.join("defaults").join("schedules.toml"),
            "default",
        )
        .unwrap();
        fs::write(deployment.join("schedules.toml"), "override").unwrap();

        let paths = ConventionalPaths::discover(&config, &deployment);
        assert_eq!(paths.schedule_files.len(), 2);
        assert!(
            paths.schedule_files[0]
                .to_string_lossy()
                .contains("defaults")
        );
        assert!(
            paths.schedule_files[1]
                .to_string_lossy()
                .contains("deployment")
        );
    }
}
