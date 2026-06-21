use crate::config::GlobalPackages;
use crate::shims::is_executable_file;
use anyhow::{Context, Result};
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command as ProcessCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedGlobalBackend {
    Mise,
    Aube,
}

pub fn resolve_backend(
    setting: GlobalPackages,
    packages: &[String],
) -> Result<ResolvedGlobalBackend> {
    let setting = env_backend_override()?.unwrap_or(setting);
    match setting {
        GlobalPackages::Mise => Ok(ResolvedGlobalBackend::Mise),
        GlobalPackages::Aube => Ok(ResolvedGlobalBackend::Aube),
        GlobalPackages::Auto => resolve_auto_backend(packages),
    }
}

fn env_backend_override() -> Result<Option<GlobalPackages>> {
    let Some(value) = env::var_os("AUBESHIM_GLOBAL_PACKAGES_BACKEND") else {
        return Ok(None);
    };
    let value = value.to_string_lossy();
    match value.as_ref() {
        "auto" => Ok(Some(GlobalPackages::Auto)),
        "mise" => Ok(Some(GlobalPackages::Mise)),
        "aube" => Ok(Some(GlobalPackages::Aube)),
        _ => anyhow::bail!(
            "invalid AUBESHIM_GLOBAL_PACKAGES_BACKEND `{value}`; expected `auto`, `mise`, or `aube`"
        ),
    }
}

fn resolve_auto_backend(packages: &[String]) -> Result<ResolvedGlobalBackend> {
    let aube = resolve_aube();
    let mise = resolve_mise();
    let aube_any = aube
        .as_ref()
        .map(|aube| aube_has_global_packages(aube.as_os_str()))
        .transpose()?
        .unwrap_or(false);
    let mise_any = mise
        .as_ref()
        .map(|mise| mise_has_global_npm_packages(mise.as_os_str()))
        .transpose()?
        .unwrap_or(false);
    let aube_names = aube
        .as_ref()
        .map(|aube| aube_global_package_names(aube.as_os_str()))
        .transpose()?
        .unwrap_or_default();
    let mise_names = mise
        .as_ref()
        .map(|mise| mise_global_npm_package_names(mise.as_os_str()))
        .transpose()?
        .unwrap_or_default();
    let package_refs = packages.iter().map(String::as_str).collect::<Vec<_>>();

    Ok(resolve_auto_backend_from_state(
        &package_refs,
        aube_any,
        mise_any,
        |package| aube_names.iter().any(|name| name == package),
        |package| {
            mise_names
                .iter()
                .any(|name| name == &format!("npm:{package}"))
        },
    ))
}

pub fn normalize_package_name(package: &str) -> String {
    package.strip_prefix("npm:").unwrap_or(package).to_owned()
}

fn aube_has_global_packages(aube: &OsStr) -> Result<bool> {
    Ok(!aube_global_package_names(aube)?.is_empty())
}

fn mise_has_global_npm_packages(mise: &OsStr) -> Result<bool> {
    Ok(!mise_global_npm_package_names(mise)?.is_empty())
}

fn aube_global_package_names(aube: &OsStr) -> Result<Vec<String>> {
    let output = ProcessCommand::new(aube)
        .args(["list", "-g", "--json"])
        .output()
        .with_context(|| format!("failed to run {}", Path::new(aube).display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("name").and_then(|name| name.as_str()))
        .map(str::to_owned)
        .collect())
}

fn mise_global_npm_package_names(mise: &OsStr) -> Result<Vec<String>> {
    let output = ProcessCommand::new(mise)
        .args(["ls", "-g", "--json"])
        .output()
        .with_context(|| format!("failed to run {}", Path::new(mise).display()))?;
    if !output.status.success() {
        return Ok(Vec::new());
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    Ok(value
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(name, _)| name.starts_with("npm:"))
        .map(|(name, _)| name.clone())
        .collect())
}

fn resolve_aube() -> Option<OsString> {
    env::var_os("AUBESHIM_AUBE").or_else(|| path_which("aube"))
}

fn resolve_mise() -> Option<OsString> {
    path_which("mise")
}

fn path_which(name: &str) -> Option<OsString> {
    let paths = env::var_os("PATH")?;
    for dir in env::split_paths(&paths) {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate.into_os_string());
        }
    }
    None
}

pub(crate) fn resolve_auto_backend_from_state(
    packages: &[&str],
    aube_any: bool,
    _mise_any: bool,
    package_in_aube: impl Fn(&str) -> bool,
    package_in_mise: impl Fn(&str) -> bool,
) -> ResolvedGlobalBackend {
    if !packages.is_empty() {
        for package in packages {
            let normalized = normalize_package_name(package);
            let in_aube = package_in_aube(&normalized);
            let in_mise = package_in_mise(&normalized);
            if in_aube && !in_mise {
                return ResolvedGlobalBackend::Aube;
            }
            if in_mise && !in_aube {
                return ResolvedGlobalBackend::Mise;
            }
            if in_aube && in_mise {
                return ResolvedGlobalBackend::Aube;
            }
        }
    }

    if aube_any {
        ResolvedGlobalBackend::Aube
    } else {
        ResolvedGlobalBackend::Mise
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = env::var_os(key);
            env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => env::set_var(self.key, value),
                None => env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn env_backend_override_accepts_auto_mise_and_aube() {
        let _guard = EnvVarGuard::set("AUBESHIM_GLOBAL_PACKAGES_BACKEND", "auto");
        assert_eq!(env_backend_override().unwrap(), Some(GlobalPackages::Auto));

        let _guard = EnvVarGuard::set("AUBESHIM_GLOBAL_PACKAGES_BACKEND", "mise");
        assert_eq!(env_backend_override().unwrap(), Some(GlobalPackages::Mise));

        let _guard = EnvVarGuard::set("AUBESHIM_GLOBAL_PACKAGES_BACKEND", "aube");
        assert_eq!(env_backend_override().unwrap(), Some(GlobalPackages::Aube));
    }

    #[test]
    fn auto_prefers_named_package_owner() {
        assert_eq!(
            resolve_auto_backend_from_state(
                &["prettier"],
                false,
                true,
                |_| false,
                |name| name == "prettier"
            ),
            ResolvedGlobalBackend::Mise
        );
        assert_eq!(
            resolve_auto_backend_from_state(
                &["prettier"],
                true,
                false,
                |name| name == "prettier",
                |_| false
            ),
            ResolvedGlobalBackend::Aube
        );
    }

    #[test]
    fn auto_prefers_aube_when_package_is_in_both_stores() {
        assert_eq!(
            resolve_auto_backend_from_state(
                &["prettier"],
                true,
                true,
                |name| name == "prettier",
                |name| name == "prettier"
            ),
            ResolvedGlobalBackend::Aube
        );
    }

    #[test]
    fn auto_uses_store_presence_when_package_is_unknown() {
        assert_eq!(
            resolve_auto_backend_from_state(&["prettier"], true, false, |_| false, |_| false),
            ResolvedGlobalBackend::Aube
        );
        assert_eq!(
            resolve_auto_backend_from_state(&["prettier"], false, true, |_| false, |_| false),
            ResolvedGlobalBackend::Mise
        );
    }

    #[test]
    fn auto_defaults_to_mise_when_no_globals_exist() {
        assert_eq!(
            resolve_auto_backend_from_state(&[], false, false, |_| false, |_| false),
            ResolvedGlobalBackend::Mise
        );
        assert_eq!(
            resolve_auto_backend_from_state(&["prettier"], false, false, |_| false, |_| false),
            ResolvedGlobalBackend::Mise
        );
    }

    #[test]
    fn auto_prefers_aube_for_unfiltered_commands_when_aube_has_globals() {
        assert_eq!(
            resolve_auto_backend_from_state(&[], true, true, |_| false, |_| false),
            ResolvedGlobalBackend::Aube
        );
    }

    #[test]
    fn normalize_package_name_strips_mise_prefix() {
        assert_eq!(normalize_package_name("npm:prettier"), "prettier");
        assert_eq!(
            normalize_package_name("npm:@biomejs/biome"),
            "@biomejs/biome"
        );
    }
}
