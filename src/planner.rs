mod bun;
mod dlx;
mod npm;
mod pnpm;
mod yarn;

use crate::config::{should_shim, Config};
use crate::home_dir;
use crate::shims::ShimTool;
use anyhow::Result;
use std::ffi::OsString;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: Target,
    pub args: Vec<OsString>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Aube,
    Mise,
    RealBun,
    RealBunx,
    RealNpm,
    RealNpx,
    RealPnpm,
    RealPnpx,
    RealPnx,
    RealYarn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobalPackageAction {
    Use,
    Unuse,
}

pub fn plan_for(tool: ShimTool, args: &[OsString]) -> Plan {
    match tool {
        ShimTool::Bun => bun::plan(args),
        ShimTool::Bunx => bun::plan_bunx(args),
        ShimTool::Npm => npm::plan(args),
        ShimTool::Npx => dlx::plan_npx(args),
        ShimTool::Pnpm => pnpm::plan(args),
        ShimTool::Pnpx => dlx::plan_pnpm_dlx(args, Target::RealPnpx),
        ShimTool::Pnx => dlx::plan_pnpm_dlx(args, Target::RealPnx),
        ShimTool::Yarn => yarn::plan(args),
    }
}

pub fn plan_for_config(
    tool: ShimTool,
    args: &[OsString],
    config: &Config,
    cwd: &Path,
) -> Result<Plan> {
    if should_shim(config, cwd)? {
        return Ok(plan_for(tool, args));
    }

    Ok(Plan {
        target: real_target_for(tool),
        args: args.to_vec(),
    })
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

fn plan_mise_global_outdated(args: &[OsString]) -> Plan {
    let mut out = vec![
        OsString::from("outdated"),
        OsString::from("--bump"),
        OsString::from("-C"),
        home_dir().into_os_string(),
    ];
    out.extend(translate_global_outdated_args(args));
    Plan {
        target: Target::Mise,
        args: out,
    }
}

fn plan_mise_global_package_action(action: GlobalPackageAction, args: &[OsString]) -> Option<Plan> {
    let packages = translate_global_package_args(args);
    if packages.is_empty() {
        return None;
    }

    let mut out = vec![
        OsString::from(match action {
            GlobalPackageAction::Use => "use",
            GlobalPackageAction::Unuse => "unuse",
        }),
        OsString::from("-g"),
    ];
    out.extend(packages);
    Some(Plan {
        target: Target::Mise,
        args: out,
    })
}

fn command_index(args: &[OsString]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            return None;
        }
        if arg.starts_with("--") {
            let name = long_flag_name(&arg);
            if global_flag_takes_value(name) && !arg.contains('=') {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 {
            if short_global_flag_takes_value(&arg) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        return Some(i);
    }
    None
}

fn has_global_marker(args: &[OsString]) -> bool {
    args.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        is_global_marker(&arg)
    })
}

fn translate_global_outdated_args(args: &[OsString]) -> Vec<OsString> {
    args.iter()
        .filter_map(|arg| {
            let s = arg.to_string_lossy();
            match s.as_ref() {
                "-g" | "--global" => None,
                "--json" => Some(arg.clone()),
                value if value.starts_with("--global=") => None,
                value if value.starts_with('-') => None,
                package => Some(OsString::from(format!("npm:{package}"))),
            }
        })
        .collect()
}

fn translate_global_package_args(args: &[OsString]) -> Vec<OsString> {
    let mut packages = Vec::new();
    let mut i = 0;
    let mut literal = false;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if !literal && arg == "--" {
            literal = true;
            i += 1;
            continue;
        }
        if !literal && is_global_marker(&arg) {
            i += 1;
            continue;
        }
        if !literal && arg.starts_with("--") {
            let name = long_flag_name(&arg);
            if global_package_flag_takes_value(name) && !arg.contains('=') {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if !literal && arg.starts_with('-') && arg.len() > 1 {
            if short_global_package_flag_takes_value(&arg) {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        packages.push(OsString::from(format!("npm:{arg}")));
        i += 1;
    }
    packages
}

fn translate_omit_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--omit" {
            let value = args.get(i + 1)?.to_string_lossy();
            push_omit_translation(&mut out, &value)?;
            i += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--omit=") {
            push_omit_translation(&mut out, value)?;
            i += 1;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    Some(out)
}

fn prepare_exec_args(args: &[OsString]) -> Vec<OsString> {
    let mut out = args.to_vec();
    if let Some(command_idx) = command_index(&out) {
        if command_idx + 1 < out.len() {
            out.insert(command_idx + 1, OsString::from("--"));
        }
    }
    out
}

fn push_omit_translation(out: &mut Vec<OsString>, value: &str) -> Option<()> {
    for item in value.split(',') {
        match item {
            "dev" => out.push(OsString::from("--prod")),
            "optional" => out.push(OsString::from("--no-optional")),
            _ => return None,
        }
    }
    Some(())
}

fn is_global_marker(arg: &str) -> bool {
    arg == "-g" || arg == "--global" || arg.starts_with("--global=")
}

fn global_flag_takes_value(name: &str) -> bool {
    matches!(
        name,
        "cache" | "color" | "loglevel" | "prefix" | "registry" | "userconfig"
    )
}

fn global_package_flag_takes_value(name: &str) -> bool {
    global_flag_takes_value(name) || install_flag_takes_value(name)
}

fn install_flag_takes_value(name: &str) -> bool {
    matches!(
        name,
        "cache"
            | "cpu"
            | "include"
            | "install-strategy"
            | "libc"
            | "loglevel"
            | "omit"
            | "os"
            | "prefix"
            | "registry"
            | "save-prefix"
            | "tag"
            | "userconfig"
            | "workspace"
    )
}

fn short_global_flag_takes_value(arg: &str) -> bool {
    matches!(arg, "-C")
}

fn short_global_package_flag_takes_value(arg: &str) -> bool {
    short_global_flag_takes_value(arg) || short_install_flag_takes_value(arg)
}

fn short_install_flag_takes_value(arg: &str) -> bool {
    matches!(arg, "-C" | "-w")
}

fn long_flag_name(arg: &str) -> &str {
    arg.trim_start_matches("--")
        .split_once('=')
        .map(|(name, _)| name)
        .unwrap_or_else(|| arg.trim_start_matches("--"))
}
