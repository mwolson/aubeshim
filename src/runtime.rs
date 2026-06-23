use crate::config::{load_config, should_shim};
use crate::planner::{aube_args_need_npm_path, plan_for_config, Plan, Target};
use crate::shims::{default_shim_dir, is_executable_file, ShimTool};
use anyhow::{anyhow, bail, Context, Result};
use std::cmp::Ordering;
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};

const MIN_MISE_VERSION: &str = "2026.5.6";

pub fn exec_shim(tool: ShimTool, args: &[OsString]) -> Result<()> {
    if is_version_request(args) {
        let status = if should_runtime_shim()? {
            run_version(tool, args)?
        } else {
            run_external_plan(None, real_plan_for(tool, args))?
        };
        std::process::exit(exit_code(status));
    }

    let plan = runtime_plan_for(tool, args)?;
    let code = run_plan(Some(tool), plan)?;
    std::process::exit(code);
}

fn is_version_request(args: &[OsString]) -> bool {
    args.len() == 1 && matches!(args[0].to_str(), Some("--version" | "-v"))
}

fn run_version(tool: ShimTool, args: &[OsString]) -> Result<ExitStatus> {
    let real_tool = resolve_real_tool(tool)?;
    let output = ProcessCommand::new(&real_tool)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {}", PathBuf::from(&real_tool).display()))?;

    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;

    if output.status.success() {
        if !output.stdout.is_empty() && !output.stdout.ends_with(b"\n") {
            println!();
        }
        let aube_version = aube_version()?;
        println!(
            "(shimmed by aubeshim v{} to aube v{aube_version})",
            env!("CARGO_PKG_VERSION")
        );
    }

    Ok(output.status)
}
fn runtime_plan_for(tool: ShimTool, args: &[OsString]) -> Result<Plan> {
    let config = load_config()?;
    let cwd = env::current_dir().context("could not determine current directory")?;
    plan_for_config(tool, args, &config, &cwd)
}

fn should_runtime_shim() -> Result<bool> {
    let config = load_config()?;
    let cwd = env::current_dir().context("could not determine current directory")?;
    should_shim(&config, &cwd)
}

fn real_plan_for(tool: ShimTool, args: &[OsString]) -> Plan {
    Plan {
        target: real_target_for(tool),
        args: args.to_vec(),
    }
}

fn real_target_for(tool: ShimTool) -> Target {
    match tool {
        ShimTool::Bun => Target::RealBun,
        ShimTool::Bunx => Target::RealBunx,
        ShimTool::Npm => Target::RealNpm,
        ShimTool::Npx => Target::RealNpx,
        ShimTool::Pnpm => Target::RealPnpm,
        ShimTool::Pnpx => Target::RealPnpx,
        ShimTool::Pnx => Target::RealPnx,
        ShimTool::Yarn => Target::RealYarn,
    }
}

fn run_plan(tool: Option<ShimTool>, plan: Plan) -> Result<i32> {
    match plan.target {
        Target::MiseGlobalList => return run_mise_global_list(&plan.args),
        Target::MiseGlobalOutdated => return run_mise_global_outdated(&plan.args),
        _ => {}
    }

    Ok(exit_code(run_external_plan(tool, plan)?))
}

fn run_external_plan(tool: Option<ShimTool>, plan: Plan) -> Result<ExitStatus> {
    let program = resolve_target(plan.target)?;
    let mut cmd = ProcessCommand::new(&program);
    cmd.args(&plan.args);

    if should_inject_aube_npm_path(plan.target, &plan.args) {
        if let Some(npm) = resolve_real_npm()? {
            cmd.env("AUBE_NPM_PATH", npm);
        }
    }

    if let Some(tool) = tool {
        if let Some((key, value)) =
            npm_compat_node_linker_env(tool, plan.target, node_linker_env_is_set())
        {
            cmd.env(key, value);
        }
    }

    cmd.status()
        .with_context(|| format!("failed to run {}", PathBuf::from(program).display()))
}

/// Whether `run_external_plan` should set `AUBE_NPM_PATH` for PLAN.
pub(crate) fn should_inject_aube_npm_path(target: Target, args: &[OsString]) -> bool {
    should_inject_aube_npm_path_with_env(target, args, env::var_os("AUBE_NPM_PATH").is_some())
}

pub(crate) fn should_inject_aube_npm_path_with_env(
    target: Target,
    args: &[OsString],
    npm_path_already_set: bool,
) -> bool {
    matches!(target, Target::Aube) && !npm_path_already_set && aube_args_need_npm_path(args)
}

fn run_mise_global_list(args: &[OsString]) -> Result<i32> {
    let mise = resolve_mise()?.ok_or_else(missing_mise_error)?;
    let package_args = package_args(args);
    if !package_args.is_empty() {
        let mut mise_args = vec![OsString::from("ls"), OsString::from("-g")];
        mise_args.extend(args.iter().cloned());
        return run_passthrough(&mise, &mise_args);
    }

    let tools = read_global_mise_npm_tools(&mise)?;
    if has_json_arg(args) {
        println!("{}", serde_json::to_string_pretty(&tools)?);
        return Ok(0);
    }

    let names = tool_names(&tools);
    if names.is_empty() {
        return Ok(0);
    }

    let mut mise_args = vec![OsString::from("ls"), OsString::from("-g")];
    mise_args.extend(names);
    run_passthrough(&mise, &mise_args)
}

fn run_mise_global_outdated(args: &[OsString]) -> Result<i32> {
    let mise = resolve_mise()?.ok_or_else(missing_mise_error)?;
    let tools = read_global_mise_npm_tools(&mise)?;
    let names = tool_names(&tools);
    if names.is_empty() {
        if has_json_arg(args) {
            println!("{{}}");
        } else {
            println!("mise All tools are up to date");
        }
        return Ok(0);
    }

    let mut mise_args = vec![
        OsString::from("outdated"),
        OsString::from("--bump"),
        OsString::from("-C"),
        env::temp_dir().into_os_string(),
    ];
    mise_args.extend(args.iter().cloned());
    mise_args.extend(names);
    run_passthrough(&mise, &mise_args)
}

fn run_passthrough(program: &OsStr, args: &[OsString]) -> Result<i32> {
    let status = ProcessCommand::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to run {}", PathBuf::from(program).display()))?;
    Ok(exit_code(status))
}

fn read_global_mise_npm_tools(mise: &OsStr) -> Result<serde_json::Map<String, serde_json::Value>> {
    let output = ProcessCommand::new(mise)
        .args(["ls", "-g", "--json"])
        .output()
        .with_context(|| format!("failed to run {}", PathBuf::from(mise).display()))?;

    if !output.status.success() {
        io::stdout().write_all(&output.stdout)?;
        io::stderr().write_all(&output.stderr)?;
        bail!("failed to list global mise tools");
    }

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("mise ls -g --json output was invalid")?;
    let tools = value
        .as_object()
        .ok_or_else(|| anyhow!("mise ls -g --json output was not an object"))?;

    Ok(tools
        .iter()
        .filter(|(name, _)| name.starts_with("npm:"))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect())
}

fn tool_names(tools: &serde_json::Map<String, serde_json::Value>) -> Vec<OsString> {
    tools.keys().map(OsString::from).collect()
}

fn package_args(args: &[OsString]) -> Vec<&OsString> {
    args.iter()
        .filter(|arg| !arg.to_string_lossy().starts_with("--"))
        .collect()
}

fn has_json_arg(args: &[OsString]) -> bool {
    args.iter().any(|arg| arg == "--json")
}

pub(crate) fn npm_compat_node_linker_env(
    tool: ShimTool,
    target: Target,
    explicit_node_linker_env: bool,
) -> Option<(&'static str, &'static str)> {
    if tool == ShimTool::Npm && target == Target::Aube && !explicit_node_linker_env {
        return Some(("AUBE_NODE_LINKER", "hoisted"));
    }
    None
}

fn node_linker_env_is_set() -> bool {
    env::var_os("AUBE_NODE_LINKER").is_some()
        || env::var_os("NPM_CONFIG_NODE_LINKER").is_some()
        || env::var_os("npm_config_node_linker").is_some()
}

fn resolve_target(target: Target) -> Result<OsString> {
    match target {
        Target::Aube => resolve_aube()?.ok_or_else(|| missing_tool_error("aube", "AUBESHIM_AUBE")),
        Target::Mise => resolve_mise()?.ok_or_else(missing_mise_error),
        Target::MiseGlobalList | Target::MiseGlobalOutdated => {
            unreachable!("custom mise targets are handled before target resolution")
        }
        Target::RealBun => {
            resolve_real_bun()?.ok_or_else(|| missing_tool_error("real bun", "AUBESHIM_REAL_BUN"))
        }
        Target::RealBunx => resolve_real_bunx()?
            .ok_or_else(|| missing_tool_error("real bunx", "AUBESHIM_REAL_BUNX")),
        Target::RealNpm => {
            resolve_real_npm()?.ok_or_else(|| missing_tool_error("real npm", "AUBESHIM_REAL_NPM"))
        }
        Target::RealNpx => {
            resolve_real_npx()?.ok_or_else(|| missing_tool_error("real npx", "AUBESHIM_REAL_NPX"))
        }
        Target::RealPnpm => resolve_real_pnpm()?
            .ok_or_else(|| missing_tool_error("real pnpm", "AUBESHIM_REAL_PNPM")),
        Target::RealPnpx => resolve_real_pnpx()?
            .ok_or_else(|| missing_tool_error("real pnpx", "AUBESHIM_REAL_PNPX")),
        Target::RealPnx => {
            resolve_real_pnx()?.ok_or_else(|| missing_tool_error("real pnx", "AUBESHIM_REAL_PNX"))
        }
        Target::RealYarn => resolve_real_yarn()?
            .ok_or_else(|| missing_tool_error("real yarn", "AUBESHIM_REAL_YARN")),
    }
}

fn resolve_real_tool(tool: ShimTool) -> Result<OsString> {
    match tool {
        ShimTool::Bun => {
            resolve_real_bun()?.ok_or_else(|| missing_tool_error("real bun", "AUBESHIM_REAL_BUN"))
        }
        ShimTool::Bunx => resolve_real_bunx()?
            .ok_or_else(|| missing_tool_error("real bunx", "AUBESHIM_REAL_BUNX")),
        ShimTool::Npm => {
            resolve_real_npm()?.ok_or_else(|| missing_tool_error("real npm", "AUBESHIM_REAL_NPM"))
        }
        ShimTool::Npx => {
            resolve_real_npx()?.ok_or_else(|| missing_tool_error("real npx", "AUBESHIM_REAL_NPX"))
        }
        ShimTool::Pnpm => resolve_real_pnpm()?
            .ok_or_else(|| missing_tool_error("real pnpm", "AUBESHIM_REAL_PNPM")),
        ShimTool::Pnpx => resolve_real_pnpx()?
            .ok_or_else(|| missing_tool_error("real pnpx", "AUBESHIM_REAL_PNPX")),
        ShimTool::Pnx => {
            resolve_real_pnx()?.ok_or_else(|| missing_tool_error("real pnx", "AUBESHIM_REAL_PNX"))
        }
        ShimTool::Yarn => resolve_real_yarn()?
            .ok_or_else(|| missing_tool_error("real yarn", "AUBESHIM_REAL_YARN")),
    }
}

fn aube_version() -> Result<String> {
    let aube = resolve_aube()?.ok_or_else(|| missing_tool_error("aube", "AUBESHIM_AUBE"))?;
    let output = ProcessCommand::new(&aube)
        .arg("--version")
        .output()
        .with_context(|| format!("failed to run {}", PathBuf::from(&aube).display()))?;

    if !output.status.success() {
        bail!(
            "failed to check aube version with {}",
            PathBuf::from(aube).display()
        );
    }

    let stdout = String::from_utf8(output.stdout).context("aube --version output was not UTF-8")?;
    version_from_output(&stdout)
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("could not parse aube version from `{}`", stdout.trim()))
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

fn resolve_real_bunx() -> Result<Option<OsString>> {
    resolve_tool("bunx", "AUBESHIM_REAL_BUNX", path_which_excluding_shims)
}

fn resolve_real_npm() -> Result<Option<OsString>> {
    resolve_tool("npm", "AUBESHIM_REAL_NPM", path_which_excluding_shims)
}

fn resolve_real_npx() -> Result<Option<OsString>> {
    resolve_tool("npx", "AUBESHIM_REAL_NPX", path_which_excluding_shims)
}

fn resolve_real_pnpm() -> Result<Option<OsString>> {
    resolve_tool("pnpm", "AUBESHIM_REAL_PNPM", path_which_excluding_shims)
}

fn resolve_real_pnpx() -> Result<Option<OsString>> {
    resolve_tool("pnpx", "AUBESHIM_REAL_PNPX", path_which_excluding_shims)
}

fn resolve_real_pnx() -> Result<Option<OsString>> {
    resolve_tool("pnx", "AUBESHIM_REAL_PNX", path_which_excluding_shims)
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
    version_from_output(output)
}

pub(crate) fn version_from_output(output: &str) -> Option<&str> {
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

#[cfg(test)]
mod inject_aube_npm_path_tests {
    use super::{should_inject_aube_npm_path_with_env, Target};
    use crate::planner::plan_for;
    use crate::planner::test_support::os;
    use crate::shims::ShimTool;

    #[test]
    fn native_aube_plans_skip_npm_path_injection() {
        for (tool, args) in [
            (ShimTool::Npm, &["bin"][..]),
            (ShimTool::Npm, &["install"][..]),
            (ShimTool::Npm, &["run", "build"][..]),
            (ShimTool::Pnpm, &["install", "--frozen-lockfile"][..]),
            (ShimTool::Yarn, &["install"][..]),
        ] {
            let plan = plan_for(tool, &os(args));
            assert_eq!(plan.target, Target::Aube, "tool={tool:?} args={args:?}");
            assert!(
                !should_inject_aube_npm_path_with_env(plan.target, &plan.args, false),
                "tool={tool:?} args={args:?}"
            );
        }
    }

    #[test]
    fn real_npm_plans_skip_aube_npm_path_injection() {
        for args in [&["publish"][..], &["whoami"][..], &["owner", "ls"][..]] {
            let plan = plan_for(ShimTool::Npm, &os(args));
            assert_eq!(plan.target, Target::RealNpm);
            assert!(!should_inject_aube_npm_path_with_env(
                plan.target,
                &plan.args,
                false
            ));
        }
    }

    #[test]
    fn compat_aube_plans_inject_npm_path_when_unset() {
        for (tool, args) in [
            (ShimTool::Pnpm, &["whoami"][..]),
            (ShimTool::Pnpm, &["--filter", "app", "whoami"][..]),
            (ShimTool::Yarn, &["--cwd", "packages/app", "whoami"][..]),
        ] {
            let plan = plan_for(tool, &os(args));
            assert_eq!(plan.target, Target::Aube, "tool={tool:?} args={args:?}");
            assert!(
                should_inject_aube_npm_path_with_env(plan.target, &plan.args, false),
                "tool={tool:?} args={args:?}"
            );
        }
    }

    #[test]
    fn existing_aube_npm_path_is_not_overwritten() {
        let plan = plan_for(ShimTool::Pnpm, &os(&["whoami"]));
        assert!(!should_inject_aube_npm_path_with_env(
            plan.target,
            &plan.args,
            true
        ));
    }

    #[test]
    fn script_named_like_compat_command_does_not_inject_npm_path() {
        let plan = plan_for(ShimTool::Npm, &os(&["run", "whoami"]));
        assert_eq!(plan.target, Target::Aube);
        assert!(!should_inject_aube_npm_path_with_env(
            plan.target,
            &plan.args,
            false
        ));
    }
}
