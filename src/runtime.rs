use crate::config::load_config;
use crate::planner::{plan_for_config, Plan, Target};
use crate::shims::{default_shim_dir, is_executable_file, ShimTool};
use anyhow::{anyhow, bail, Context, Result};
use std::cmp::Ordering;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};

const MIN_MISE_VERSION: &str = "2026.5.6";

pub fn exec_shim(tool: ShimTool, args: &[OsString]) -> Result<()> {
    let plan = runtime_plan_for(tool, args)?;
    let status = run_plan(plan)?;
    std::process::exit(exit_code(status));
}
fn runtime_plan_for(tool: ShimTool, args: &[OsString]) -> Result<Plan> {
    let config = load_config()?;
    let cwd = env::current_dir().context("could not determine current directory")?;
    plan_for_config(tool, args, &config, &cwd)
}

fn run_plan(plan: Plan) -> Result<ExitStatus> {
    let program = resolve_target(plan.target)?;
    let mut cmd = ProcessCommand::new(&program);
    cmd.args(&plan.args);

    if matches!(plan.target, Target::Aube) && env::var_os("AUBE_NPM_PATH").is_none() {
        if let Some(npm) = resolve_real_npm()? {
            cmd.env("AUBE_NPM_PATH", npm);
        }
    }

    cmd.status()
        .with_context(|| format!("failed to run {}", PathBuf::from(program).display()))
}

fn resolve_target(target: Target) -> Result<OsString> {
    match target {
        Target::Aube => resolve_aube()?.ok_or_else(|| missing_tool_error("aube", "AUBESHIM_AUBE")),
        Target::Mise => resolve_mise()?.ok_or_else(missing_mise_error),
        Target::RealBun => {
            resolve_real_bun()?.ok_or_else(|| missing_tool_error("real bun", "AUBESHIM_REAL_BUN"))
        }
        Target::RealNpm => {
            resolve_real_npm()?.ok_or_else(|| missing_tool_error("real npm", "AUBESHIM_REAL_NPM"))
        }
        Target::RealPnpm => resolve_real_pnpm()?
            .ok_or_else(|| missing_tool_error("real pnpm", "AUBESHIM_REAL_PNPM")),
        Target::RealYarn => resolve_real_yarn()?
            .ok_or_else(|| missing_tool_error("real yarn", "AUBESHIM_REAL_YARN")),
    }
}

fn resolve_mise() -> Result<Option<OsString>> {
    let Some(mise) = path_which("mise") else {
        return Ok(None);
    };
    ensure_supported_mise(&mise)?;
    Ok(Some(mise))
}

fn resolve_aube() -> Result<Option<OsString>> {
    resolve_tool("aube", "AUBESHIM_AUBE", path_which)
}

fn resolve_real_bun() -> Result<Option<OsString>> {
    resolve_tool("bun", "AUBESHIM_REAL_BUN", path_which_excluding_shims)
}

fn resolve_real_npm() -> Result<Option<OsString>> {
    resolve_tool("npm", "AUBESHIM_REAL_NPM", path_which_excluding_shims)
}

fn resolve_real_pnpm() -> Result<Option<OsString>> {
    resolve_tool("pnpm", "AUBESHIM_REAL_PNPM", path_which_excluding_shims)
}

fn resolve_real_yarn() -> Result<Option<OsString>> {
    resolve_tool("yarn", "AUBESHIM_REAL_YARN", path_which_excluding_shims)
}

fn resolve_tool(
    tool: &str,
    env_var: &str,
    path_lookup: fn(&str) -> Option<OsString>,
) -> Result<Option<OsString>> {
    if let Some(path) = env::var_os(env_var) {
        return Ok(Some(path));
    }

    if let Some(path) = mise_which(tool)? {
        return Ok(Some(path));
    }

    Ok(path_lookup(tool))
}

fn mise_which(tool: &str) -> Result<Option<OsString>> {
    let Some(mise) = path_which("mise") else {
        return Ok(None);
    };
    ensure_supported_mise(&mise)?;

    let output = ProcessCommand::new(&mise)
        .arg("which")
        .arg(tool)
        .output()
        .with_context(|| format!("failed to run {}", PathBuf::from(&mise).display()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8(output.stdout).context("mise which output was not UTF-8")?;
    let path = path.trim();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(OsString::from(path)))
    }
}

fn ensure_supported_mise(mise: &OsStr) -> Result<()> {
    let output = ProcessCommand::new(mise)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to run {}", PathBuf::from(mise).display()))?;
    if !output.status.success() {
        bail!(
            "failed to check mise version with {}",
            PathBuf::from(mise).display()
        );
    }

    let stdout = String::from_utf8(output.stdout).context("mise --version output was not UTF-8")?;
    let version = mise_version_from_output(&stdout)
        .ok_or_else(|| anyhow!("could not parse mise version from `{}`", stdout.trim()))?;
    if compare_dotted_versions(version, MIN_MISE_VERSION) == Ordering::Less {
        return Err(unsupported_mise_error(version));
    }

    Ok(())
}

pub(crate) fn mise_version_from_output(output: &str) -> Option<&str> {
    output
        .split_whitespace()
        .find(|token| token.chars().any(|ch| ch.is_ascii_digit()))
}

pub(crate) fn unsupported_mise_error(version: &str) -> anyhow::Error {
    anyhow!(
        "mise {version} is too old for aubeshim; install mise >= {MIN_MISE_VERSION}. Arch Linux's mise package may lag behind the version needed for aube support."
    )
}

pub(crate) fn compare_dotted_versions(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('.').map(parse_version_part);
    let mut right_parts = right.split('.').map(parse_version_part);

    loop {
        match (left_parts.next(), right_parts.next()) {
            (None, None) => return Ordering::Equal,
            (left, right) => {
                let ordering = left.unwrap_or(0).cmp(&right.unwrap_or(0));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
        }
    }
}

fn parse_version_part(part: &str) -> u32 {
    part.chars()
        .skip_while(|ch| !ch.is_ascii_digit())
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

pub(crate) fn missing_tool_error(tool: &str, env_var: &str) -> anyhow::Error {
    let mise_hint = if command_on_path("mise") {
        "aubeshim also tried `mise which`; make sure the tool is installed with mise"
    } else {
        "mise is not on PATH; install mise first if you expect aubeshim to find tools through mise"
    };
    anyhow!("could not find {tool}; set {env_var} to an absolute path, install it another way, or install it with mise. {mise_hint}")
}

fn missing_mise_error() -> anyhow::Error {
    anyhow!(
        "could not find mise; install mise >= {MIN_MISE_VERSION} to use aubeshim global npm tool support"
    )
}

fn path_which(name: &str) -> Option<OsString> {
    path_which_with_filter(name, |_| true)
}

fn command_on_path(name: &str) -> bool {
    path_which(name).is_some()
}

fn path_which_excluding_shims(name: &str) -> Option<OsString> {
    let shim_dir = default_shim_dir();
    path_which_with_filter(name, |candidate| {
        candidate.parent() != Some(shim_dir.as_path())
    })
}

fn path_which_with_filter(name: &str, keep: impl Fn(&Path) -> bool) -> Option<OsString> {
    let paths = env::var_os("PATH")?;
    for dir in env::split_paths(&paths) {
        let candidate = dir.join(name);
        if keep(&candidate) && is_executable_file(&candidate) {
            return Some(candidate.into_os_string());
        }
    }
    None
}
fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }

    1
}
