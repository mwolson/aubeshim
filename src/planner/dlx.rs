use super::{long_flag_name, prepare_exec_args, Target};
use crate::planner::Plan;
use std::ffi::OsString;

pub(super) fn plan_npx(args: &[OsString]) -> Plan {
    let translated = translate_npx_args(args);
    Plan {
        target: translated.target.unwrap_or(Target::Aube),
        args: translated.args,
    }
}

pub(super) fn plan_pnpm_dlx(args: &[OsString], real_target: Target) -> Plan {
    plan_pnpm_dlx_with_prefix(&[], args, real_target, args)
}

pub(super) fn plan_pnpm_dlx_with_prefix(
    prefix: &[OsString],
    args: &[OsString],
    real_target: Target,
    real_args: &[OsString],
) -> Plan {
    let Some(translated) = translate_pnpm_dlx_args(args) else {
        return Plan {
            target: real_target,
            args: real_args.to_vec(),
        };
    };

    let mut out = Vec::with_capacity(prefix.len() + translated.len());
    out.extend_from_slice(prefix);
    out.extend(translated);
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn translate_pnpm_dlx_args(args: &[OsString]) -> Option<Vec<OsString>> {
    let mut out = Vec::with_capacity(args.len() + 1);
    out.push(OsString::from("dlx"));

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            out.extend_from_slice(&args[i..]);
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            out.extend_from_slice(&args[i..]);
            break;
        }
        if arg == "--allow-build" || arg.starts_with("--allow-build=") {
            return None;
        }
        if arg == "-s" {
            out.push(OsString::from("--silent"));
            i += 1;
            continue;
        }
        if pnpm_dlx_flag_takes_value(&arg) && !arg.contains('=') {
            out.push(args[i].clone());
            if let Some(value) = args.get(i + 1) {
                out.push(value.clone());
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }

    Some(out)
}

struct TranslatedNpx {
    target: Option<Target>,
    args: Vec<OsString>,
}

fn translate_npx_args(args: &[OsString]) -> TranslatedNpx {
    let mut out = Vec::with_capacity(args.len() + 1);
    let mut for_exec = Vec::with_capacity(args.len());
    let mut no_install = false;
    out.push(OsString::from("dlx"));

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            out.extend_from_slice(&args[i + 1..]);
            for_exec.extend_from_slice(&args[i + 1..]);
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            out.extend_from_slice(&args[i..]);
            for_exec.extend_from_slice(&args[i..]);
            break;
        }

        match arg.as_ref() {
            "-y" | "--yes" => {
                i += 1;
            }
            "-s" => {
                out.push(OsString::from("--silent"));
                i += 1;
            }
            "--no-install" => {
                no_install = true;
                i += 1;
            }
            "-c" | "--call" => {
                out.push(OsString::from("-c"));
                if let Some(value) = args.get(i + 1) {
                    out.push(value.clone());
                    for_exec.push(value.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-p" | "--package" => {
                out.push(args[i].clone());
                if let Some(value) = args.get(i + 1) {
                    out.push(value.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "-w" | "--workspace" | "--workspaces" | "--include-workspace-root" => {
                return TranslatedNpx {
                    target: Some(Target::RealNpx),
                    args: args.to_vec(),
                };
            }
            _ if arg.starts_with("--package=") => {
                out.push(args[i].clone());
                i += 1;
            }
            _ if arg.starts_with("--call=") => {
                out.push(OsString::from("-c"));
                out.push(OsString::from(
                    arg.strip_prefix("--call=").expect("prefix matched"),
                ));
                for_exec.push(OsString::from(
                    arg.strip_prefix("--call=").expect("prefix matched"),
                ));
                i += 1;
            }
            _ if npx_aube_dlx_flag_takes_value(&arg) && !arg.contains('=') => {
                out.push(args[i].clone());
                if let Some(value) = args.get(i + 1) {
                    out.push(value.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ if npx_aube_dlx_flag(&arg) => {
                out.push(args[i].clone());
                i += 1;
            }
            _ => {
                return TranslatedNpx {
                    target: Some(Target::RealNpx),
                    args: args.to_vec(),
                };
            }
        }
    }

    if no_install {
        let mut exec_args = vec![OsString::from("exec"), OsString::from("--no-install")];
        exec_args.extend(prepare_exec_args(&for_exec));
        return TranslatedNpx {
            target: None,
            args: exec_args,
        };
    }

    TranslatedNpx {
        target: None,
        args: out,
    }
}

fn pnpm_dlx_flag_takes_value(arg: &str) -> bool {
    matches!(
        long_flag_name(arg),
        "allow-build" | "package" | "reporter" | "registry"
    )
}

fn npx_aube_dlx_flag(arg: &str) -> bool {
    if matches!(
        arg,
        "--silent" | "-v" | "--verbose" | "--color" | "--no-color"
    ) {
        return true;
    }

    if !arg.starts_with("--") {
        return false;
    }

    matches!(
        long_flag_name(arg),
        "fetch-retries"
            | "fetch-retry-factor"
            | "fetch-retry-maxtimeout"
            | "fetch-retry-mintimeout"
            | "fetch-timeout"
            | "frozen-lockfile"
            | "loglevel"
            | "no-frozen-lockfile"
            | "prefer-frozen-lockfile"
            | "registry"
            | "reporter"
    )
}

fn npx_aube_dlx_flag_takes_value(arg: &str) -> bool {
    if matches!(arg, "-c" | "-p") {
        return true;
    }

    if !arg.starts_with("--") {
        return false;
    }

    matches!(
        long_flag_name(arg),
        "fetch-retries"
            | "fetch-retry-factor"
            | "fetch-retry-maxtimeout"
            | "fetch-retry-mintimeout"
            | "fetch-timeout"
            | "loglevel"
            | "package"
            | "registry"
            | "reporter"
    )
}
