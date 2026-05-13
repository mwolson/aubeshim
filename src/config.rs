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
    pub ignore: Vec<String>,
    pub shim: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            default: true,
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
        compile_repo_glob(pattern)?;
    }
    Ok(())
}

pub(crate) fn should_shim(config: &Config, cwd: &Path) -> Result<bool> {
    if !config.enabled {
        return Ok(false);
    }

    let repo = repo_dir(cwd);
    if matches_repo_glob(&config.ignore, &repo)? {
        return Ok(false);
    }
    if matches_repo_glob(&config.shim, &repo)? {
        return Ok(true);
    }
    Ok(config.default)
}

fn matches_repo_glob(patterns: &[String], repo: &Path) -> Result<bool> {
    for pattern in patterns {
        if matches_repo_glob_pattern(pattern, repo)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn matches_repo_glob_pattern(pattern: &str, repo: &Path) -> Result<bool> {
    let pattern = expand_home(pattern);
    if matches_expanded_repo_glob(&pattern, repo)? {
        return Ok(true);
    }
    if let Some(base) = pattern.strip_suffix("/**") {
        return matches_expanded_repo_glob(base, repo);
    }
    Ok(false)
}

fn matches_expanded_repo_glob(pattern: &str, repo: &Path) -> Result<bool> {
    Ok(Pattern::new(pattern)
        .with_context(|| format!("invalid repo glob `{pattern}`"))?
        .matches_path_with(repo, repo_glob_match_options()))
}

fn compile_repo_glob(pattern: &str) -> Result<Pattern> {
    let pattern = expand_home(pattern);
    Pattern::new(&pattern).with_context(|| format!("invalid repo glob `{pattern}`"))
}

fn repo_glob_match_options() -> MatchOptions {
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

fn repo_dir(cwd: &Path) -> PathBuf {
    let mut dir = cwd;
    loop {
        if dir.join(".git").exists() {
            return dir.to_path_buf();
        }
        let Some(parent) = dir.parent() else {
            return cwd.to_path_buf();
        };
        dir = parent;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_leading_tilde_in_repo_globs() {
        let home = home_dir();

        assert_eq!(expand_home("~"), home.to_string_lossy());
        assert_eq!(
            expand_home("~/devel/alairo/**"),
            home.join("devel/alairo/**").to_string_lossy()
        );
        assert_eq!(expand_home("/tmp/~"), "/tmp/~");
    }

    #[test]
    fn trailing_double_star_matches_base_repo_dir() {
        let repo = home_dir().join("devel/alairo");

        assert!(matches_repo_glob_pattern("~/devel/alairo/**", &repo).unwrap());
    }
}
