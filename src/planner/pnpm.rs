use super::{
    command_index, dlx, has_global_marker, plan_mise_global_outdated,
    plan_mise_global_package_action, GlobalPackageAction, Plan, Target,
};
use std::ffi::OsString;

pub(super) fn plan(args: &[OsString]) -> Plan {
    if let Some(command_idx) = command_index(args) {
        let command = args[command_idx].to_string_lossy().to_ascii_lowercase();
        if command == "dlx" {
            return dlx::plan_pnpm_dlx_with_prefix(
                &args[..command_idx],
                &args[command_idx + 1..],
                Target::RealPnpm,
                args,
            );
        }
        if command == "outdated" && has_global_marker(args) {
            return plan_mise_global_outdated(&args[command_idx + 1..]);
        }
        if has_global_marker(args) {
            if let Some(action) = pnpm_global_package_action(&command) {
                return plan_mise_global_package_action(action, &args[command_idx + 1..])
                    .unwrap_or_else(|| Plan {
                        target: Target::RealPnpm,
                        args: args.to_vec(),
                    });
            }
        }
    }

    Plan {
        target: Target::Aube,
        args: args.to_vec(),
    }
}

fn pnpm_global_package_action(command: &str) -> Option<GlobalPackageAction> {
    match command {
        "add" | "i" | "install" => Some(GlobalPackageAction::Use),
        "remove" | "rm" | "uninstall" => Some(GlobalPackageAction::Unuse),
        _ => None,
    }
}
