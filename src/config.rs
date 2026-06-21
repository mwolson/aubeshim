use crate::home_dir;
use anyhow::{Context, Result};
use glob::{MatchOptions, Pattern};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub default: bool,
    pub global_packages: GlobalPackages,
    pub ignore: Vec<String>,
    pub shim: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GlobalPackages {
    Auto,
    Mise,
    Aube,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            default: true,
            global_packages: GlobalPackages::Auto,
            ignore: Vec::new(),
            shim: Vec::new(),
        }
    }
}

pub fn default_config_path() -> PathBuf {
    if let Some(path) = env::var_os("AUBESHIM_CONFIG") {
        return PathBuf::from(path);
    }
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("aubeshim")
            .join("config.toml");
    }
    home_dir().join(".config/aubeshim/config.toml")
}

pub fn load_config() -> Result<Config> {
    let path = default_config_path();
    if !path.exists() {
        return Ok(Config::default());
    }

    let content =
        fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
    parse_config(&content, &path)
}
pub(crate) fn parse_config(content: &str, path: &Path) -> Result<Config> {
    let config: Config =
        toml::from_str(content).with_context(|| format!("could not parse {}", path.display()))?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &Config) -> Result<()> {
    for pattern in config.ignore.iter().chain(config.shim.iter()) {
        compile_dir_glob(pattern)?;
    }
    Ok(())
}

pub(crate) fn should_shim(config: &Config, cwd: &Path) -> Result<bool> {
    if !config.enabled {
        return Ok(false);
    }

    if matches_dir_glob(&config.ignore, cwd)? {
        return Ok(false);
    }
    if matches_dir_glob(&config.shim, cwd)? {
        return Ok(true);
    }
    Ok(config.default)
}

fn matches_dir_glob(patterns: &[String], cwd: &Path) -> Result<bool> {
    for pattern in patterns {
        if matches_dir_glob_pattern(pattern, cwd)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn matches_dir_glob_pattern(pattern: &str, cwd: &Path) -> Result<bool> {
    let pattern = expand_home(pattern);
    if matches_expanded_dir_glob(&pattern, cwd)? {
        return Ok(true);
    }
    if let Some(base) = pattern.strip_suffix("/**") {
        return matches_expanded_dir_glob(base, cwd);
    }
    Ok(false)
}

fn matches_expanded_dir_glob(pattern: &str, cwd: &Path) -> Result<bool> {
    let pattern = Pattern::new(pattern).with_context(|| format!("invalid dir glob `{pattern}`"))?;
    Ok(cwd
        .ancestors()
        .any(|dir| pattern.matches_path_with(dir, dir_glob_match_options())))
}

fn compile_dir_glob(pattern: &str) -> Result<Pattern> {
    let pattern = expand_home(pattern);
    Pattern::new(&pattern).with_context(|| format!("invalid dir glob `{pattern}`"))
}

fn dir_glob_match_options() -> MatchOptions {
    MatchOptions {
        case_sensitive: true,
        require_literal_separator: true,
        require_literal_leading_dot: false,
    }
}

fn expand_home(pattern: &str) -> String {
    if pattern == "~" {
        return home_dir().to_string_lossy().into_owned();
    }
    if let Some(rest) = pattern.strip_prefix("~/") {
        return home_dir().join(rest).to_string_lossy().into_owned();
    }
    pattern.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_leading_tilde_in_dir_globs() {
        let home = home_dir();

        assert_eq!(expand_home("~"), home.to_string_lossy());
        assert_eq!(
            expand_home("~/devel/alairo/**"),
            home.join("devel/alairo/**").to_string_lossy()
        );
        assert_eq!(expand_home("/tmp/~"), "/tmp/~");
    }

    #[test]
    fn dir_globs_match_current_dir_or_ancestors() {
        let root = home_dir().join("devel/alairo/iow/amp/amp-mobile");
        let cwd = root.join("components/forms");

        assert!(matches_dir_glob_pattern(&root.to_string_lossy(), &cwd).unwrap());
    }

    #[test]
    fn single_star_dir_globs_match_descendants_of_selected_dirs() {
        let cwd = home_dir().join("devel/alairo/iow/amp/amp-mobile/components");

        assert!(matches_dir_glob_pattern("~/devel/alairo/iow/amp/*", &cwd).unwrap());
    }

    #[test]
    fn trailing_double_star_matches_base_dir() {
        let cwd = home_dir().join("devel/alairo");

        assert!(matches_dir_glob_pattern("~/devel/alairo/**", &cwd).unwrap());
    }
}
