use super::{
    command_index, has_global_marker, long_flag_name, plan_mise_global_outdated,
    plan_mise_global_package_action, prepare_exec_args, translate_omit_args, GlobalPackageAction,
    Plan, Target,
};
use std::ffi::{OsStr, OsString};
use std::path::Path;

pub(super) fn plan(args: &[OsString]) -> Plan {
    let Some(command_idx) = command_index(args) else {
        return Plan {
            target: Target::RealBun,
            args: args.to_vec(),
        };
    };
    let command = args[command_idx].to_string_lossy().to_ascii_lowercase();
    let prefix = &args[..command_idx];
    let rest = &args[command_idx + 1..];

    let Some(command) = normalize_bun_command(&command) else {
        return Plan {
            target: Target::RealBun,
            args: args.to_vec(),
        };
    };

    if command == "outdated" && has_global_marker(args) {
        return plan_mise_global_outdated(rest);
    }

    if command == "run" && bun_run_uses_real_bun(prefix, rest) {
        return Plan {
            target: Target::RealBun,
            args: args.to_vec(),
        };
    }

    if command == "dlx" {
        if bun_dlx_uses_real_bun(prefix, rest) {
            return Plan {
                target: Target::RealBun,
                args: args.to_vec(),
            };
        }
        return plan_dlx(prefix, rest);
    }

    if has_global_marker(args) {
        if let Some(action) = bun_global_package_action(command) {
            return plan_mise_global_package_action(action, rest).unwrap_or_else(|| Plan {
                target: Target::RealBun,
                args: args.to_vec(),
            });
        }
    }

    let mut out = Vec::with_capacity(args.len());
    out.extend_from_slice(prefix);
    out.push(OsString::from(command));
    if command == "install" {
        let Some(translated) = translate_omit_args(rest) else {
            return Plan {
                target: Target::RealBun,
                args: args.to_vec(),
            };
        };
        out.extend(translated);
    } else {
        out.extend_from_slice(rest);
    }
    Plan {
        target: Target::Aube,
        args: out,
    }
}

pub(super) fn plan_bunx(args: &[OsString]) -> Plan {
    if bun_dlx_uses_real_bun(&[], args) {
        return Plan {
            target: Target::RealBunx,
            args: args.to_vec(),
        };
    }

    plan_dlx(&[], args)
}

fn plan_dlx(prefix: &[OsString], rest: &[OsString]) -> Plan {
    let mut no_install = false;
    let translated_prefix = translate_dlx_prefix(prefix, &mut no_install);
    let translated_rest = translate_dlx_rest(rest, &mut no_install);

    let mut out = Vec::with_capacity(translated_prefix.len() + translated_rest.for_dlx.len() + 3);
    out.extend(translated_prefix);
    if no_install {
        out.push(OsString::from("exec"));
        out.push(OsString::from("--no-install"));
        out.extend(prepare_aube_exec_args(&translated_rest.for_exec));
    } else {
        out.push(OsString::from("dlx"));
        out.extend(translated_rest.for_dlx);
    }
    Plan {
        target: Target::Aube,
        args: out,
    }
}

fn translate_dlx_prefix(args: &[OsString], no_install: &mut bool) -> Vec<OsString> {
    args.iter()
        .filter_map(|arg| {
            let value = arg.to_string_lossy();
            if is_dlx_no_install_flag(&value) {
                *no_install = true;
                return None;
            }
            if is_dlx_install_flag(&value) {
                return None;
            }
            Some(arg.clone())
        })
        .collect()
}

struct TranslatedDlxRest {
    for_dlx: Vec<OsString>,
    for_exec: Vec<OsString>,
}

fn translate_dlx_rest(args: &[OsString], no_install: &mut bool) -> TranslatedDlxRest {
    let mut for_dlx = Vec::with_capacity(args.len());
    let mut for_exec = Vec::with_capacity(args.len());
    let mut i = 0;

    while i < args.len() {
        let arg = args[i].to_string_lossy();
        if arg == "--" {
            for_dlx.extend_from_slice(&args[i..]);
            for_exec.extend_from_slice(&args[i..]);
            break;
        }
        if !arg.starts_with('-') || arg == "-" {
            for_dlx.extend_from_slice(&args[i..]);
            for_exec.extend_from_slice(&args[i..]);
            break;
        }
        if is_dlx_no_install_flag(&arg) {
            *no_install = true;
            i += 1;
            continue;
        }
        if is_dlx_install_flag(&arg) {
            i += 1;
            continue;
        }
        if arg == "--package" || arg == "-p" {
            for_dlx.push(args[i].clone());
            if let Some(value) = args.get(i + 1) {
                for_dlx.push(value.clone());
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if arg.starts_with("--package=") {
            for_dlx.push(args[i].clone());
            i += 1;
            continue;
        }

        for_dlx.push(args[i].clone());
        for_exec.push(args[i].clone());
        i += 1;
    }

    TranslatedDlxRest { for_dlx, for_exec }
}

fn prepare_aube_exec_args(args: &[OsString]) -> Vec<OsString> {
    prepare_exec_args(args)
}

fn bun_dlx_uses_real_bun(prefix: &[OsString], rest: &[OsString]) -> bool {
    if prefix.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        is_bun_dlx_runtime_flag(&arg)
    }) {
        return true;
    }

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].to_string_lossy();
        if arg == "--" {
            return false;
        }
        if !arg.starts_with('-') || arg == "-" {
            return false;
        }
        if is_bun_dlx_runtime_flag(&arg) {
            return true;
        }
        if bun_dlx_flag_takes_value(&arg) && !arg.contains('=') {
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

fn is_bun_dlx_runtime_flag(arg: &str) -> bool {
    arg == "--bun" || arg == "-b"
}

fn bun_dlx_flag_takes_value(arg: &str) -> bool {
    arg == "-p" || matches!(long_flag_name(arg), "package")
}

fn is_dlx_no_install_flag(arg: &str) -> bool {
    arg == "--no-install"
}

fn is_dlx_install_flag(arg: &str) -> bool {
    arg == "-i" || arg.starts_with("--install=")
}

fn bun_global_package_action(command: &str) -> Option<GlobalPackageAction> {
    match command {
        "add" | "install" => Some(GlobalPackageAction::Use),
        "remove" => Some(GlobalPackageAction::Unuse),
        _ => None,
    }
}

fn normalize_bun_command(command: &str) -> Option<&'static str> {
    match command {
        "add" => Some("add"),
        "i" | "install" => Some("install"),
        "link" => Some("link"),
        "outdated" => Some("outdated"),
        "publish" => Some("publish"),
        "remove" | "rm" => Some("remove"),
        "run" => Some("run"),
        "unlink" => Some("unlink"),
        "update" | "upgrade" => Some("update"),
        "x" => Some("dlx"),
        _ => None,
    }
}

fn bun_run_uses_real_bun(prefix: &[OsString], rest: &[OsString]) -> bool {
    if prefix.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        is_bun_runtime_flag(&arg)
    }) {
        return true;
    }

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].to_string_lossy();
        if arg == "--" {
            return false;
        }
        if !arg.starts_with('-') || arg == "-" {
            return looks_like_file_entrypoint(&arg);
        }
        if is_bun_runtime_flag(&arg) {
            return true;
        }
        i += 1;
    }
    false
}

fn is_bun_runtime_flag(arg: &str) -> bool {
    if !arg.starts_with('-') || arg == "-" {
        return false;
    }
    let name = long_flag_name(arg);
    matches!(
        name,
        "bun"
            | "conditions"
            | "config"
            | "console-depth"
            | "cpu-prof"
            | "cpu-prof-dir"
            | "cpu-prof-interval"
            | "cpu-prof-md"
            | "cpu-prof-name"
            | "cwd"
            | "define"
            | "dns-result-order"
            | "drop"
            | "elide-lines"
            | "env-file"
            | "eval"
            | "expose-gc"
            | "extension-order"
            | "feature"
            | "fetch-preconnect"
            | "heap-prof"
            | "heap-prof-dir"
            | "heap-prof-md"
            | "heap-prof-name"
            | "hot"
            | "if-present"
            | "import"
            | "inspect"
            | "inspect-brk"
            | "inspect-wait"
            | "jsx-factory"
            | "jsx-fragment"
            | "jsx-import-source"
            | "jsx-runtime"
            | "jsx-side-effects"
            | "loader"
            | "main-fields"
            | "max-http-header-size"
            | "no-addons"
            | "no-clear-screen"
            | "no-deprecation"
            | "no-env-file"
            | "no-exit-on-error"
            | "no-install"
            | "no-macros"
            | "parallel"
            | "port"
            | "preload"
            | "prefer-latest"
            | "prefer-offline"
            | "preserve-symlinks"
            | "preserve-symlinks-main"
            | "print"
            | "redis-preconnect"
            | "require"
            | "shell"
            | "smol"
            | "sql-preconnect"
            | "throw-deprecation"
            | "title"
            | "tsconfig-override"
            | "unhandled-rejections"
            | "use-bundled-ca"
            | "use-openssl-ca"
            | "use-system-ca"
            | "user-agent"
            | "watch"
            | "workspaces"
            | "zero-fill-buffers"
    ) || matches!(arg, "-b" | "-e" | "-i" | "-p" | "-r")
}

fn looks_like_file_entrypoint(arg: &str) -> bool {
    arg == "-"
        || arg.starts_with("./")
        || arg.starts_with("../")
        || arg.starts_with('/')
        || matches!(
            Path::new(arg).extension().and_then(OsStr::to_str),
            Some("cjs" | "cts" | "js" | "jsx" | "mjs" | "mts" | "tsx" | "ts")
        )
}
