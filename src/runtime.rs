use crate::config::{load_config, should_hoist, should_shim};
use crate::home_dir;
use crate::planner::{plan_for_config, Plan, Target};
use crate::shims::{default_shim_dir, is_executable_file, ShimTool};
use anyhow::{anyhow, bail, Context, Result};
use std::cmp::Ordering;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitStatus};

const MIN_MISE_VERSION: &str = "2026.5.6";

pub fn exec_shim(tool: ShimTool, args: &[OsString]) -> Result<()> {
    let config = load_config()?;
    let cwd = env::current_dir().context("could not determine current directory")?;

    if is_version_request(args) {
        let status = if should_shim(&config, &cwd)? {
            run_version(tool, args)?
        } else {
            // Passthrough version has no aube linker env.
            run_external_plan(None, real_plan_for(tool, args), false)?
        };
        std::process::exit(exit_code(status));
    }

    // One config + cwd snapshot for routing and linker env so plan and exec
    // cannot disagree if the config file changes mid-invocation.
    let plan = plan_for_config(tool, args, &config, &cwd)?;
    let force_hoisted = should_hoist(&config, &cwd)?;
    let code = run_plan(Some(tool), plan, force_hoisted)?;
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

fn run_plan(tool: Option<ShimTool>, plan: Plan, force_hoisted: bool) -> Result<i32> {
    match plan.target {
        Target::MiseGlobalList => return run_mise_global_list(&plan.args),
        Target::MiseGlobalOutdated => return run_mise_global_outdated(&plan.args),
        _ => {}
    }

    Ok(exit_code(run_external_plan(tool, plan, force_hoisted)?))
}

fn run_external_plan(
    tool: Option<ShimTool>,
    plan: Plan,
    force_hoisted: bool,
) -> Result<ExitStatus> {
    let program = resolve_target(plan.target)?;
    let mut cmd = ProcessCommand::new(&program);
    cmd.args(&plan.args);

    if let Some(tool) = tool {
        if let Some((key, value)) =
            aube_node_linker_env(tool, plan.target, node_linker_env_is_set(), force_hoisted)
        {
            cmd.env(key, value);
        }
        // Script commands can auto-install, so apply the safe default to every Aube-backed plan.
        if safe_package_import_method_env(tool, plan.target, false).is_some() {
            if let Some((key, value)) = safe_package_import_method_env(
                tool,
                plan.target,
                package_import_method_is_explicit(&plan.args)?,
            ) {
                cmd.env(key, value);
            }
        }
    }

    cmd.status()
        .with_context(|| format!("failed to run {}", PathBuf::from(program).display()))
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

pub(crate) fn aube_node_linker_env(
    tool: ShimTool,
    target: Target,
    explicit_node_linker_env: bool,
    force_hoisted: bool,
) -> Option<(&'static str, &'static str)> {
    if target != Target::Aube || explicit_node_linker_env {
        return None;
    }
    if !matches!(
        tool,
        ShimTool::Bun | ShimTool::Npm | ShimTool::Pnpm | ShimTool::Yarn
    ) {
        return None;
    }
    // npm defaults to hoisted for aube-backed plans. Other package managers only
    // hoist when the cwd matches a configured `hoisted` directory glob.
    if tool == ShimTool::Npm || force_hoisted {
        return Some(("AUBE_NODE_LINKER", "hoisted"));
    }
    None
}

fn node_linker_env_is_set() -> bool {
    env::var_os("AUBE_NODE_LINKER").is_some()
        || env::var_os("NPM_CONFIG_NODE_LINKER").is_some()
        || env::var_os("npm_config_node_linker").is_some()
}

pub(crate) fn safe_package_import_method_env(
    tool: ShimTool,
    target: Target,
    explicit_package_import_method: bool,
) -> Option<(&'static str, &'static str)> {
    if matches!(
        tool,
        ShimTool::Bun | ShimTool::Npm | ShimTool::Pnpm | ShimTool::Yarn
    ) && target == Target::Aube
        && !explicit_package_import_method
    {
        return Some(("AUBE_PACKAGE_IMPORT_METHOD", "clone-or-copy"));
    }
    None
}

fn package_import_method_is_explicit(args: &[OsString]) -> Result<bool> {
    if package_import_method_arg_is_set(args) || package_import_method_env_is_set() {
        return Ok(true);
    }

    let cwd = env::current_dir().context("could not determine current directory")?;
    Ok(package_import_method_config_is_set(&cwd))
}

fn package_import_method_arg_is_set(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        arg == "--package-import-method" || arg.starts_with("--package-import-method=")
    })
}

fn package_import_method_env_is_set() -> bool {
    [
        "AUBE_PACKAGE_IMPORT_METHOD",
        "NPM_CONFIG_PACKAGE_IMPORT_METHOD",
        "npm_config_package_import_method",
    ]
    .iter()
    .any(|key| env::var_os(key).is_some_and(|value| !value.is_empty()))
}

fn package_import_method_config_is_set(cwd: &Path) -> bool {
    let user_npmrc = home_dir().join(".npmrc");
    if npmrc_declares_package_import_method(&user_npmrc) {
        return true;
    }

    let config_home = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"));
    if toml_declares_package_import_method(&config_home.join("aube/config.toml")) {
        return true;
    }

    let Some(root) = find_aube_project_root(cwd) else {
        return false;
    };
    project_declares_package_import_method(&root)
}

fn project_declares_package_import_method(root: &Path) -> bool {
    npmrc_declares_package_import_method(&root.join(".npmrc"))
        || yaml_declares_package_import_method(&root.join("aube-workspace.yaml"))
        || yaml_declares_package_import_method(&root.join("pnpm-workspace.yaml"))
        || toml_declares_package_import_method(&root.join(".config/aube/config.toml"))
}

fn find_aube_project_root(cwd: &Path) -> Option<PathBuf> {
    let mut package_root = None;
    for dir in cwd.ancestors() {
        if dir.join("aube-workspace.yaml").is_file()
            || dir.join("pnpm-workspace.yaml").is_file()
            || package_json_declares_workspaces(&dir.join("package.json"))
        {
            return Some(dir.to_path_buf());
        }
        if package_root.is_none() && dir.join("package.json").is_file() {
            package_root = Some(dir.to_path_buf());
        }
    }
    package_root
}

fn package_json_declares_workspaces(path: &Path) -> bool {
    let Some(content) = read_optional_config(path) else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(&content)
        .ok()
        .is_some_and(|manifest| manifest.get("workspaces").is_some())
}

fn npmrc_declares_package_import_method(path: &Path) -> bool {
    let Some(content) = read_optional_config(path) else {
        return false;
    };
    content.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            return false;
        }
        line.split_once('=').is_some_and(|(key, value)| {
            matches!(key.trim(), "packageImportMethod" | "package-import-method")
                && !value.trim().is_empty()
        })
    })
}

fn yaml_declares_package_import_method(path: &Path) -> bool {
    let Some(content) = read_optional_config(path) else {
        return false;
    };
    content.lines().any(|line| {
        if line.trim_start().len() != line.len() {
            return false;
        }
        let line = line.split('#').next().unwrap_or_default().trim();
        line.split_once(':').is_some_and(|(key, value)| {
            key.trim().trim_matches(['\'', '"']) == "packageImportMethod"
                && !value.trim().is_empty()
        })
    })
}

fn toml_declares_package_import_method(path: &Path) -> bool {
    let Some(content) = read_optional_config(path) else {
        return false;
    };
    toml::from_str::<toml::Value>(&content)
        .ok()
        .is_some_and(|config| {
            config.get("packageImportMethod").is_some()
                || config.get("package-import-method").is_some()
        })
}

fn read_optional_config(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    fs::read_to_string(path).ok()
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
    for env_var in ["AUBESHIM_REAL_NPM", "AUBE_NPM_PATH", "NPM_CONFIG_NPM_PATH"] {
        if let Some(path) = env::var_os(env_var) {
            return Ok(Some(path));
        }
    }

    if let Some(path) = mise_which("npm")? {
        if is_acceptable_real_tool(Path::new(&path)) {
            return Ok(Some(path));
        }
    }

    Ok(path_which_excluding_shims("npm"))
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
        // mise which normally returns a real install path. Reject dispatcher
        // shims if a broken or recursive setup points back into aubeshim/mise.
        if is_acceptable_real_tool(Path::new(&path)) {
            return Ok(Some(path));
        }
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

/// Locate a real package manager on PATH, skipping dispatcher shim directories.
///
/// Aubeshim's own shims and mise's shims both re-enter tool resolution. If
/// `mise which` fails and PATH falls back to either of those, aubeshim can
/// recurse forever (aubeshim -> mise shim -> aubeshim). Prefer a real install
/// path, or fail closed with no candidate.
fn path_which_excluding_shims(name: &str) -> Option<OsString> {
    let aubeshim_shim_dir = default_shim_dir();
    let mise_shim_dir = default_mise_shim_dir();
    path_which_with_filter(name, |candidate| {
        is_acceptable_real_tool_in_dirs(candidate, &aubeshim_shim_dir, &mise_shim_dir)
    })
}

fn default_mise_shim_dir() -> PathBuf {
    if let Some(dir) = env::var_os("MISE_DATA_DIR") {
        return PathBuf::from(dir).join("shims");
    }
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(data_home).join("mise").join("shims");
    }
    home_dir().join(".local/share/mise/shims")
}

fn is_acceptable_real_tool(path: &Path) -> bool {
    is_acceptable_real_tool_in_dirs(path, &default_shim_dir(), &default_mise_shim_dir())
}

fn is_acceptable_real_tool_in_dirs(
    path: &Path,
    aubeshim_shim_dir: &Path,
    mise_shim_dir: &Path,
) -> bool {
    if let Some(parent) = path.parent() {
        if paths_refer_to_same_dir(parent, aubeshim_shim_dir)
            || paths_refer_to_same_dir(parent, mise_shim_dir)
        {
            return false;
        }
    }
    !is_current_aubeshim_executable(path)
}

fn paths_refer_to_same_dir(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn is_current_aubeshim_executable(path: &Path) -> bool {
    let Ok(current) = env::current_exe() else {
        return false;
    };
    match (fs::canonicalize(path), fs::canonicalize(current)) {
        (Ok(candidate), Ok(current)) => candidate == current,
        _ => false,
    }
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
mod tests {
    use super::*;

    #[test]
    fn npm_aube_plans_default_to_hoisted_node_linker() {
        assert_eq!(
            aube_node_linker_env(ShimTool::Npm, Target::Aube, false, false),
            Some(("AUBE_NODE_LINKER", "hoisted"))
        );
        assert_eq!(
            aube_node_linker_env(ShimTool::Npm, Target::Aube, true, false),
            None
        );
        assert_eq!(
            aube_node_linker_env(ShimTool::Npm, Target::RealNpm, false, false),
            None
        );
    }

    #[test]
    fn pattern_hoist_applies_to_all_package_manager_shims() {
        for tool in [ShimTool::Bun, ShimTool::Npm, ShimTool::Pnpm, ShimTool::Yarn] {
            assert_eq!(
                aube_node_linker_env(tool, Target::Aube, false, true),
                Some(("AUBE_NODE_LINKER", "hoisted"))
            );
            assert_eq!(aube_node_linker_env(tool, Target::Aube, true, true), None);
        }
    }

    #[test]
    fn non_npm_tools_keep_default_linker_without_pattern() {
        for tool in [ShimTool::Bun, ShimTool::Pnpm, ShimTool::Yarn] {
            assert_eq!(aube_node_linker_env(tool, Target::Aube, false, false), None);
        }
    }

    #[test]
    fn runner_shims_and_real_targets_skip_node_linker_env() {
        for (tool, target, force_hoisted) in [
            (ShimTool::Npx, Target::Aube, true),
            (ShimTool::Pnpx, Target::Aube, true),
            (ShimTool::Pnx, Target::Aube, true),
            (ShimTool::Bunx, Target::Aube, true),
            (ShimTool::Pnpm, Target::RealPnpm, true),
            (ShimTool::Yarn, Target::RealYarn, true),
        ] {
            assert_eq!(
                aube_node_linker_env(tool, target, false, force_hoisted),
                None
            );
        }
    }

    #[test]
    fn force_hoisted_reads_config_globs_for_cwd() {
        use crate::config::{parse_config, should_hoist, Config};

        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("t3code-parent");
        let t3code = parent.join("t3code");
        let trees = parent.join("trees/feature-branch");
        let tasks = parent.join("tasks/foo");
        fs::create_dir_all(t3code.join("apps/mobile")).unwrap();
        fs::create_dir_all(&trees).unwrap();
        fs::create_dir_all(&tasks).unwrap();

        let config = parse_config(
            &format!(
                "hoisted = [\n  \"{}/**\",\n  \"{}/**\",\n]\n",
                t3code.display(),
                parent.join("trees").display()
            ),
            Path::new("/tmp/aubeshim-hoisted-test.toml"),
        )
        .unwrap();

        // Mirrors exec_shim: one config snapshot drives both routing and linker env.
        assert!(should_hoist(&config, &t3code.join("apps/mobile")).unwrap());
        assert_eq!(
            aube_node_linker_env(
                ShimTool::Pnpm,
                Target::Aube,
                false,
                should_hoist(&config, &t3code.join("apps/mobile")).unwrap(),
            ),
            Some(("AUBE_NODE_LINKER", "hoisted"))
        );
        assert!(should_hoist(&config, &trees).unwrap());
        assert!(!should_hoist(&config, &tasks).unwrap());
        assert_eq!(
            aube_node_linker_env(
                ShimTool::Pnpm,
                Target::Aube,
                false,
                should_hoist(&config, &tasks).unwrap(),
            ),
            None
        );
        assert!(!should_hoist(&config, &parent).unwrap());
        assert!(!should_hoist(&Config::default(), &t3code).unwrap());
    }

    #[test]
    fn shimmed_package_managers_default_to_safe_imports() {
        for tool in [ShimTool::Bun, ShimTool::Npm, ShimTool::Pnpm, ShimTool::Yarn] {
            assert_eq!(
                safe_package_import_method_env(tool, Target::Aube, false),
                Some(("AUBE_PACKAGE_IMPORT_METHOD", "clone-or-copy"))
            );
            assert_eq!(
                safe_package_import_method_env(tool, Target::Aube, true),
                None
            );
        }
    }

    #[test]
    fn real_tools_and_runner_shims_keep_their_import_defaults() {
        for (tool, target) in [
            (ShimTool::Bun, Target::RealBun),
            (ShimTool::Npm, Target::RealNpm),
            (ShimTool::Pnpm, Target::RealPnpm),
            (ShimTool::Yarn, Target::RealYarn),
            (ShimTool::Npx, Target::Aube),
            (ShimTool::Pnpx, Target::Aube),
            (ShimTool::Pnx, Target::Aube),
            (ShimTool::Bunx, Target::Aube),
        ] {
            assert_eq!(safe_package_import_method_env(tool, target, false), None);
        }
    }

    #[test]
    fn detects_package_import_method_cli_args() {
        assert!(package_import_method_arg_is_set(&[
            OsString::from("install"),
            OsString::from("--package-import-method=hardlink"),
        ]));
        assert!(package_import_method_arg_is_set(&[
            OsString::from("install"),
            OsString::from("--package-import-method"),
            OsString::from("copy"),
        ]));
        assert!(!package_import_method_arg_is_set(&[OsString::from(
            "install"
        )]));
    }

    #[test]
    fn detects_package_import_method_in_npmrc() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".npmrc");
        fs::write(
            &config,
            "# package-import-method=hardlink\npackageImportMethod = auto\n",
        )
        .unwrap();

        assert!(npmrc_declares_package_import_method(&config));
    }

    #[test]
    fn ignores_empty_or_noncanonical_package_import_method_in_npmrc() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".npmrc");
        fs::write(
            &config,
            "packageImportMethod=\npackageimportmethod=hardlink\n",
        )
        .unwrap();

        assert!(!npmrc_declares_package_import_method(&config));
    }

    #[test]
    fn detects_top_level_package_import_method_in_workspace_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("aube-workspace.yaml");
        fs::write(
            &config,
            "settings:\n  packageImportMethod: hardlink\npackageImportMethod: auto\n",
        )
        .unwrap();

        assert!(yaml_declares_package_import_method(&config));
    }

    #[test]
    fn ignores_nested_package_import_method_in_workspace_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("aube-workspace.yaml");
        fs::write(&config, "settings:\n  packageImportMethod: hardlink\n").unwrap();

        assert!(!yaml_declares_package_import_method(&config));
    }

    #[test]
    fn ignores_empty_package_import_method_in_workspace_yaml() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("aube-workspace.yaml");
        fs::write(&config, "packageImportMethod:\n").unwrap();

        assert!(!yaml_declares_package_import_method(&config));
    }

    #[test]
    fn detects_package_import_method_in_user_aube_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        fs::write(&config, "packageImportMethod = \"auto\"\n").unwrap();

        assert!(toml_declares_package_import_method(&config));
    }

    #[test]
    fn malformed_aube_config_does_not_block_shims() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        fs::write(&config, "packageImportMethod = [\n").unwrap();

        assert!(!toml_declares_package_import_method(&config));
    }

    #[test]
    fn finds_workspace_root_above_package_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        let package = root.join("packages/app");
        let cwd = package.join("src");
        fs::create_dir_all(&cwd).unwrap();
        fs::write(root.join("aube-workspace.yaml"), "packages: []\n").unwrap();
        fs::write(package.join("package.json"), "{}\n").unwrap();

        assert_eq!(find_aube_project_root(&cwd), Some(root));
    }

    #[test]
    fn ignores_config_above_single_package_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("parent/project");
        let cwd = root.join("src");
        fs::create_dir_all(&cwd).unwrap();
        fs::write(
            dir.path().join("parent/.npmrc"),
            "packageImportMethod=hardlink\n",
        )
        .unwrap();
        fs::write(root.join("package.json"), "{}\n").unwrap();

        let project_root = find_aube_project_root(&cwd).unwrap();
        assert_eq!(project_root, root);
        assert!(!project_declares_package_import_method(&project_root));
    }

    #[test]
    fn detects_project_aube_config() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        fs::create_dir_all(root.join(".config/aube")).unwrap();
        fs::write(root.join("package.json"), "{}\n").unwrap();
        fs::write(
            root.join(".config/aube/config.toml"),
            "packageImportMethod = \"hardlink\"\n",
        )
        .unwrap();

        assert!(project_declares_package_import_method(&root));
    }

    /// Package-manager names that can pass through to a real binary.
    const REAL_TOOL_NAMES: &[&str] = &["bun", "bunx", "npm", "npx", "pnpm", "pnpx", "pnx", "yarn"];

    struct EnvSnapshot {
        previous: Vec<(&'static str, Option<OsString>)>,
        _lock: std::sync::RwLockWriteGuard<'static, ()>,
    }

    impl EnvSnapshot {
        fn apply(pairs: &[(&'static str, Option<&str>)]) -> Self {
            let lock = crate::globals::test_env_lock::exclusive();
            let mut previous = Vec::with_capacity(pairs.len());
            for &(key, value) in pairs {
                previous.push((key, env::var_os(key)));
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
            Self {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            for (key, value) in self.previous.iter().rev() {
                match value {
                    Some(value) => env::set_var(key, value),
                    None => env::remove_var(key),
                }
            }
        }
    }

    fn write_executable(path: &Path) {
        fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(path, perms).unwrap();
        }
    }

    fn dispatcher_only_path_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let aubeshim_shims = dir.path().join("aubeshim-shims");
        let mise_data = dir.path().join("mise-data");
        let mise_shims = mise_data.join("shims");
        fs::create_dir_all(&aubeshim_shims).unwrap();
        fs::create_dir_all(&mise_shims).unwrap();

        for name in REAL_TOOL_NAMES {
            write_executable(&aubeshim_shims.join(name));
            write_executable(&mise_shims.join(name));
        }

        (dir, aubeshim_shims, mise_data)
    }

    /// Isolate resolver tests from the host environment.
    ///
    /// PATH is fully replaced (so `mise which` cannot run) and every real-tool
    /// override is cleared unless an extra pair overrides it.
    fn isolated_resolver_env(
        aubeshim_shims: &Path,
        mise_data: &Path,
        path: &std::ffi::OsStr,
        extra: &[(&'static str, Option<&str>)],
    ) -> EnvSnapshot {
        let path = path.to_str().expect("test PATH must be UTF-8");
        let mut pairs = vec![
            ("AUBESHIM_SHIM_DIR", Some(aubeshim_shims.to_str().unwrap())),
            ("MISE_DATA_DIR", Some(mise_data.to_str().unwrap())),
            ("PATH", Some(path)),
            ("AUBESHIM_REAL_BUN", None),
            ("AUBESHIM_REAL_BUNX", None),
            ("AUBESHIM_REAL_NPM", None),
            ("AUBE_NPM_PATH", None),
            ("NPM_CONFIG_NPM_PATH", None),
            ("AUBESHIM_REAL_NPX", None),
            ("AUBESHIM_REAL_PNPM", None),
            ("AUBESHIM_REAL_PNPX", None),
            ("AUBESHIM_REAL_PNX", None),
            ("AUBESHIM_REAL_YARN", None),
        ];
        for &(key, value) in extra {
            if let Some(slot) = pairs.iter_mut().find(|(existing, _)| *existing == key) {
                *slot = (key, value);
            } else {
                pairs.push((key, value));
            }
        }
        EnvSnapshot::apply(&pairs)
    }

    #[test]
    fn path_lookup_skips_aubeshim_and_mise_dispatcher_shims() {
        let (_dir, aubeshim_shims, mise_data) = dispatcher_only_path_fixture();
        let mise_shims = mise_data.join("shims");
        let path = env::join_paths([&aubeshim_shims, &mise_shims]).unwrap();
        let _env = isolated_resolver_env(&aubeshim_shims, &mise_data, &path, &[]);

        for name in REAL_TOOL_NAMES {
            assert_eq!(
                path_which_excluding_shims(name),
                None,
                "expected no real {name} when PATH only has dispatcher shims"
            );
        }

        assert!(resolve_real_bun().unwrap().is_none());
        assert!(resolve_real_bunx().unwrap().is_none());
        assert!(resolve_real_npm().unwrap().is_none());
        assert!(resolve_real_npx().unwrap().is_none());
        assert!(resolve_real_pnpm().unwrap().is_none());
        assert!(resolve_real_pnpx().unwrap().is_none());
        assert!(resolve_real_pnx().unwrap().is_none());
        assert!(resolve_real_yarn().unwrap().is_none());
    }

    #[test]
    fn path_lookup_finds_real_binary_after_dispatcher_shims() {
        let (dir, aubeshim_shims, mise_data) = dispatcher_only_path_fixture();
        let mise_shims = mise_data.join("shims");
        let real_dir = dir.path().join("real");
        fs::create_dir_all(&real_dir).unwrap();
        for name in REAL_TOOL_NAMES {
            write_executable(&real_dir.join(name));
        }

        let path = env::join_paths([&aubeshim_shims, &mise_shims, &real_dir]).unwrap();
        let _env = isolated_resolver_env(&aubeshim_shims, &mise_data, &path, &[]);

        for name in REAL_TOOL_NAMES {
            let found = path_which_excluding_shims(name).unwrap_or_else(|| {
                panic!("expected real {name} after dispatcher shims on PATH");
            });
            assert_eq!(Path::new(&found), real_dir.join(name));
        }

        assert_eq!(
            resolve_real_bun().unwrap().as_deref(),
            Some(real_dir.join("bun").as_os_str())
        );
        assert_eq!(
            resolve_real_npm().unwrap().as_deref(),
            Some(real_dir.join("npm").as_os_str())
        );
        assert_eq!(
            resolve_real_pnpm().unwrap().as_deref(),
            Some(real_dir.join("pnpm").as_os_str())
        );
        assert_eq!(
            resolve_real_yarn().unwrap().as_deref(),
            Some(real_dir.join("yarn").as_os_str())
        );
    }

    #[test]
    fn real_tool_env_override_still_wins() {
        let (_dir, aubeshim_shims, mise_data) = dispatcher_only_path_fixture();
        let mise_shims = mise_data.join("shims");
        let path = env::join_paths([&aubeshim_shims, &mise_shims]).unwrap();
        let override_bun = aubeshim_shims.join("not-used-override");
        write_executable(&override_bun);

        let _env = isolated_resolver_env(
            &aubeshim_shims,
            &mise_data,
            &path,
            &[("AUBESHIM_REAL_BUN", Some(override_bun.to_str().unwrap()))],
        );

        assert_eq!(
            resolve_real_bun().unwrap().as_deref(),
            Some(override_bun.as_os_str())
        );
    }

    #[test]
    fn rejects_dispatcher_shim_paths_as_real_tools() {
        let dir = tempfile::tempdir().unwrap();
        let aubeshim_shims = dir.path().join("aubeshim-shims");
        let mise_data = dir.path().join("mise-data");
        let mise_shims = mise_data.join("shims");
        fs::create_dir_all(&aubeshim_shims).unwrap();
        fs::create_dir_all(&mise_shims).unwrap();
        write_executable(&mise_shims.join("bun"));
        write_executable(&aubeshim_shims.join("bun"));

        let path = env::join_paths([&aubeshim_shims, &mise_shims]).unwrap();
        let _env = isolated_resolver_env(&aubeshim_shims, &mise_data, &path, &[]);

        assert!(!is_acceptable_real_tool(&mise_shims.join("bun")));
        assert!(!is_acceptable_real_tool(&aubeshim_shims.join("bun")));
    }

    #[test]
    fn resolve_real_tool_fails_closed_when_only_dispatcher_shims_exist() {
        let (_dir, aubeshim_shims, mise_data) = dispatcher_only_path_fixture();
        let mise_shims = mise_data.join("shims");
        let path = env::join_paths([&aubeshim_shims, &mise_shims]).unwrap();
        let _env = isolated_resolver_env(&aubeshim_shims, &mise_data, &path, &[]);

        for tool in [
            ShimTool::Bun,
            ShimTool::Bunx,
            ShimTool::Npm,
            ShimTool::Npx,
            ShimTool::Pnpm,
            ShimTool::Pnpx,
            ShimTool::Pnx,
            ShimTool::Yarn,
        ] {
            let err = resolve_real_tool(tool).unwrap_err().to_string();
            assert!(
                err.contains("could not find real"),
                "tool {tool:?} should fail closed, got: {err}"
            );
            assert!(
                err.contains("AUBESHIM_REAL_"),
                "tool {tool:?} error should mention override env, got: {err}"
            );
        }
    }
}
