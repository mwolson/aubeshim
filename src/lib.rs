mod cli;
mod config;
mod globals;
mod planner;
mod runtime;
mod shell;
mod shims;

pub use cli::{Cli, Command};
pub use config::{default_config_path, load_config, Config, GlobalPackages};
pub use planner::{plan_for, plan_for_config, Plan, Target};
pub use runtime::exec_shim;
pub use shell::{shell_init, Shell};
pub use shims::{
    default_shim_dir, install_shims, invocation_from_argv0, uninstall_shims, Invocation, ShimTool,
};

use std::env;
use std::path::PathBuf;

pub(crate) fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_config;
    use crate::runtime::{
        compare_dotted_versions, mise_version_from_output, missing_tool_error,
        npm_compat_node_linker_env, unsupported_mise_error, version_from_output,
    };
    use std::{cmp::Ordering, env, ffi::OsString, fs, path::Path};

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

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn config_defaults_to_shimming() {
        let repo = repo_fixture();
        let plan = plan_for_config(
            ShimTool::Npm,
            &os(&["install"]),
            &Config::default(),
            &repo.cwd,
        )
        .unwrap();

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_shimmed_version_flags_are_aube_targets() {
        let repo = repo_fixture();
        let plan = plan_for_config(
            ShimTool::Npm,
            &os(&["--version"]),
            &Config::default(),
            &repo.cwd,
        )
        .unwrap();

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["--version"]);
    }

    #[test]
    fn config_ignored_version_flags_pass_through_to_real_tool() {
        let repo = repo_fixture();
        let config = Config {
            default: false,
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["--version"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["--version"]);
    }

    #[test]
    fn config_global_disable_passes_through_to_real_tool() {
        let repo = repo_fixture();
        let config = Config {
            enabled: false,
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_global_disable_overrides_shim_glob() {
        let repo = repo_fixture();
        let config = Config {
            enabled: false,
            shim: vec![repo.root.to_string_lossy().into_owned()],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Pnpm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealPnpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_shim_glob_overrides_default_disable() {
        let repo = repo_fixture();
        let config = Config {
            default: false,
            shim: vec![repo.root.to_string_lossy().into_owned()],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Pnpm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_default_disable_passes_through_to_real_tool() {
        let repo = repo_fixture();
        let config = Config {
            default: false,
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_ignore_glob_overrides_shim_glob() {
        let repo = repo_fixture();
        let pattern = repo.root.to_string_lossy().into_owned();
        let config = Config {
            enabled: true,
            default: true,
            global_packages: GlobalPackages::Mise,
            ignore: vec![pattern.clone()],
            shim: vec![pattern],
        };
        let plan = plan_for_config(ShimTool::Yarn, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealYarn);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_globs_match_selected_ancestor_dirs() {
        let repo = repo_fixture();
        let config = Config {
            enabled: true,
            ignore: vec![format!("{}/*", repo.root.parent().unwrap().display())],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Bun, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealBun);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_single_star_glob_matches_descendants_of_selected_dirs() {
        let repo = nested_repo_fixture();
        let config = Config {
            ignore: vec![format!(
                "{}/*",
                repo.root
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .display()
            )],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn config_double_star_glob_matches_nested_dirs() {
        let repo = nested_repo_fixture();
        let config = Config {
            ignore: vec![format!(
                "{}/**",
                repo.root
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .display()
            )],
            ..Config::default()
        };
        let plan = plan_for_config(ShimTool::Npm, &os(&["install"]), &config, &repo.cwd).unwrap();

        assert_eq!(plan.target, Target::RealNpm);
        assert_eq!(strings(&plan.args), vec!["install"]);
    }

    #[test]
    fn parses_config_file() {
        let config = parse_config(
            r#"
enabled = false
default = true
global_packages = "aube"
ignore = ["~/devel/work/broken-expo"]
shim = ["~/devel/work/*"]
"#,
            Path::new("/tmp/aubeshim-config.toml"),
        )
        .unwrap();

        assert!(!config.enabled);
        assert!(config.default);
        assert_eq!(config.global_packages, GlobalPackages::Aube);
    }

    #[test]
    fn parses_auto_global_packages_and_defaults_to_auto() {
        let config = parse_config(
            r#"global_packages = "auto""#,
            Path::new("/tmp/aubeshim-config.toml"),
        )
        .unwrap();

        assert_eq!(config.global_packages, GlobalPackages::Auto);
        assert_eq!(Config::default().global_packages, GlobalPackages::Auto);
    }

    #[test]
    fn config_global_packages_aube_uses_aube_for_global_package_operations() {
        let repo = repo_fixture();
        let config = Config {
            global_packages: GlobalPackages::Aube,
            ..Config::default()
        };

        for (tool, args, expected) in [
            (
                ShimTool::Npm,
                &["install", "-g", "cowsay"][..],
                vec!["add", "-g", "cowsay"],
            ),
            (
                ShimTool::Pnpm,
                &["add", "-g", "typescript"][..],
                vec!["add", "-g", "typescript"],
            ),
            (
                ShimTool::Bun,
                &["remove", "--global", "prettier"][..],
                vec!["remove", "-g", "prettier"],
            ),
            (
                ShimTool::Yarn,
                &["remove", "-g", "eslint"][..],
                vec!["remove", "-g", "eslint"],
            ),
        ] {
            let plan = plan_for_config(tool, &os(args), &config, &repo.cwd).unwrap();

            assert_eq!(plan.target, Target::Aube);
            assert_eq!(strings(&plan.args), expected);
        }
    }

    #[test]
    fn config_global_packages_aube_routes_global_outdated_to_aube() {
        let repo = repo_fixture();
        let config = Config {
            global_packages: GlobalPackages::Aube,
            ..Config::default()
        };

        for (tool, args, expected) in [
            (
                ShimTool::Npm,
                &["outdated", "-g"][..],
                vec!["outdated", "-g"],
            ),
            (
                ShimTool::Pnpm,
                &["outdated", "--global"][..],
                vec!["outdated", "-g"],
            ),
            (
                ShimTool::Bun,
                &["outdated", "-g", "--json"][..],
                vec!["outdated", "-g", "--json"],
            ),
            (
                ShimTool::Yarn,
                &["outdated", "--global=true"][..],
                vec!["outdated", "-g"],
            ),
        ] {
            let plan = plan_for_config(tool, &os(args), &config, &repo.cwd).unwrap();

            assert_eq!(plan.target, Target::Aube);
            assert_eq!(strings(&plan.args), expected);
        }
    }

    #[test]
    fn config_global_packages_aube_routes_package_specific_outdated_to_aube() {
        let repo = repo_fixture();
        let config = Config {
            global_packages: GlobalPackages::Aube,
            ..Config::default()
        };
        let plan = plan_for_config(
            ShimTool::Npm,
            &os(&["outdated", "-g", "prettier"]),
            &config,
            &repo.cwd,
        )
        .unwrap();

        assert_eq!(plan.target, Target::Aube);
        assert_eq!(strings(&plan.args), vec!["outdated", "-g", "prettier"]);
    }

    #[test]
    fn config_global_packages_override_replaces_configured_backend() {
        let repo = repo_fixture();

        {
            let config = Config::default();
            let _guard = EnvVarGuard::set("AUBESHIM_GLOBAL_PACKAGES_BACKEND", "aube");
            let plan = plan_for_config(ShimTool::Npm, &os(&["outdated", "-g"]), &config, &repo.cwd)
                .unwrap();

            assert_eq!(plan.target, Target::Aube);
            assert_eq!(strings(&plan.args), vec!["outdated", "-g"]);
        }

        {
            let config = Config {
                global_packages: GlobalPackages::Aube,
                ..Config::default()
            };
            let _guard = EnvVarGuard::set("AUBESHIM_GLOBAL_PACKAGES_BACKEND", "mise");
            let plan = plan_for_config(ShimTool::Npm, &os(&["outdated", "-g"]), &config, &repo.cwd)
                .unwrap();

            assert_eq!(plan.target, Target::MiseGlobalOutdated);
            assert!(plan.args.is_empty());
        }
    }

    #[test]
    fn recognizes_standalone_exec_shims() {
        for (name, tool) in [
            ("bunx", ShimTool::Bunx),
            ("npx", ShimTool::Npx),
            ("pnpx", ShimTool::Pnpx),
            ("pnx", ShimTool::Pnx),
        ] {
            assert_eq!(
                invocation_from_argv0(Some(&OsString::from(name))),
                Invocation::Shim(tool)
            );
        }
    }

    #[test]
    fn npm_aube_plans_default_to_hoisted_node_linker() {
        assert_eq!(
            npm_compat_node_linker_env(ShimTool::Npm, Target::Aube, false),
            Some(("AUBE_NODE_LINKER", "hoisted"))
        );
        assert_eq!(
            npm_compat_node_linker_env(ShimTool::Npm, Target::Aube, true),
            None
        );
        assert_eq!(
            npm_compat_node_linker_env(ShimTool::Pnpm, Target::Aube, false),
            None
        );
        assert_eq!(
            npm_compat_node_linker_env(ShimTool::Npm, Target::RealNpm, false),
            None
        );
    }

    #[test]
    fn missing_tool_error_mentions_mise_and_override() {
        let message = missing_tool_error("real npm", "AUBESHIM_REAL_NPM").to_string();

        assert!(message.contains("could not find real npm"));
        assert!(message.contains("AUBESHIM_REAL_NPM"));
        assert!(message.contains("mise"));
    }

    #[test]
    fn parses_mise_version_prefix() {
        assert_eq!(
            mise_version_from_output("2026.5.6 linux-x64 (2026-05-11)"),
            Some("2026.5.6")
        );
        assert_eq!(
            mise_version_from_output("mise 2026.5.6 linux-x64"),
            Some("2026.5.6")
        );
    }

    #[test]
    fn parses_tool_version_output() {
        assert_eq!(version_from_output("aube 0.3.3\n"), Some("0.3.3"));
        assert_eq!(version_from_output("1.2.3\n"), Some("1.2.3"));
    }

    #[test]
    fn compares_dotted_versions_numerically() {
        assert_eq!(
            compare_dotted_versions("2026.5.6", "2026.5.6"),
            Ordering::Equal
        );
        assert_eq!(
            compare_dotted_versions("2026.5.10", "2026.5.6"),
            Ordering::Greater
        );
        assert_eq!(
            compare_dotted_versions("2026.5.5", "2026.5.6"),
            Ordering::Less
        );
    }

    #[test]
    fn unsupported_mise_error_mentions_arch_and_minimum() {
        let message = unsupported_mise_error("2026.5.5").to_string();

        assert!(message.contains("mise 2026.5.5 is too old"));
        assert!(message.contains("mise >= 2026.5.6"));
        assert!(message.contains("Arch Linux"));
    }

    #[test]
    fn shell_init_supports_fish() {
        let init = shell_init(
            Shell::Fish,
            Path::new("/home/me/.local/share/aubeshim/shims"),
            false,
        );

        assert!(init.contains("string match --invert"));
        assert!(init.contains("fish_add_path --path --prepend $_aubeshim_shim_dir"));
    }

    #[test]
    fn fish_activation_removes_existing_shim_entries_before_prepending() {
        let dir = tempfile::tempdir().unwrap();
        let shim_dir = dir.path().to_string_lossy();
        let path = format!("/bin:{shim_dir}:/usr/bin:{shim_dir}:/sbin");
        let output = run_shell_activation("fish", Shell::Fish, dir.path(), &path);

        assert_eq!(output, format!("{shim_dir}:/bin:/usr/bin:/sbin"));
    }

    #[test]
    fn shell_init_supports_sh() {
        let init = shell_init(
            Shell::Sh,
            Path::new("/home/me/.local/share/aubeshim/shims"),
            false,
        );

        assert!(init.contains("AUBESHIM_SHIM_DIR="));
        assert!(init.contains("export PATH"));
    }

    #[test]
    fn shell_init_supports_bash() {
        let init = shell_init(
            Shell::Bash,
            Path::new("/home/me/.local/share/aubeshim/shims"),
            false,
        );

        assert!(init.contains("PATH=\"${PATH//:$_aubeshim_shim_dir:/:}\""));
        assert!(init.contains("export PATH=\"$_aubeshim_shim_dir:$PATH\""));
    }

    #[test]
    fn bash_activation_removes_existing_shim_entries_before_prepending() {
        let dir = Path::new("/tmp/aubeshim-test");
        let output = run_shell_activation(
            "bash",
            Shell::Bash,
            dir,
            "/bin:/tmp/aubeshim-test:/usr/bin:/tmp/aubeshim-test:/sbin",
        );

        assert_eq!(output, "/tmp/aubeshim-test:/bin:/usr/bin:/sbin");
    }

    #[test]
    fn sh_activation_removes_existing_shim_entries_before_prepending() {
        let dir = Path::new("/tmp/aubeshim-test");
        let output = run_shell_activation(
            "sh",
            Shell::Sh,
            dir,
            "/bin:/tmp/aubeshim-test:/usr/bin:/tmp/aubeshim-test:/sbin",
        );

        assert_eq!(output, "/tmp/aubeshim-test:/bin:/usr/bin:/sbin");
    }

    #[test]
    fn zsh_activation_removes_existing_shim_entries_before_prepending() {
        let dir = Path::new("/tmp/aubeshim-test");
        let output = run_shell_activation(
            "zsh",
            Shell::Zsh,
            dir,
            "/bin:/tmp/aubeshim-test:/usr/bin:/tmp/aubeshim-test:/sbin",
        );

        assert_eq!(output, "/tmp/aubeshim-test:/bin:/usr/bin:/sbin");
    }

    #[cfg(unix)]
    #[test]
    fn install_and_uninstall_shims() {
        let dir = tempfile::tempdir().unwrap();
        let installed = install_shims(dir.path(), false).unwrap();
        let shim_names = ["bun", "bunx", "npm", "npx", "pnpm", "pnpx", "pnx", "yarn"];

        assert_eq!(installed.len(), shim_names.len());
        for name in shim_names {
            assert!(dir.path().join(name).is_symlink());
        }

        let removed = uninstall_shims(dir.path()).unwrap();
        assert_eq!(removed.len(), shim_names.len());
        for name in shim_names {
            assert!(!dir.path().join(name).exists());
        }
    }

    struct RepoFixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        cwd: PathBuf,
    }

    fn repo_fixture() -> RepoFixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        let cwd = root.join("packages/app");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        RepoFixture {
            _dir: dir,
            root,
            cwd,
        }
    }

    fn nested_repo_fixture() -> RepoFixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repos/work/nested/app");
        let cwd = root.join("packages/app");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        RepoFixture {
            _dir: dir,
            root,
            cwd,
        }
    }

    fn run_shell_activation(shell: &str, init_shell: Shell, dir: &Path, path: &str) -> String {
        let script = format!(
            "{}\nprintf '%s\\n' \"$PATH\"\n",
            shell_init(init_shell, dir, false)
        );
        let zdotdir = tempfile::tempdir().unwrap();
        let mut cmd = std::process::Command::new(shell);
        if shell == "fish" {
            cmd.arg("--no-config");
        }
        cmd.arg("-c").arg(script).env("PATH", path);
        if shell == "zsh" {
            cmd.env("ZDOTDIR", zdotdir.path());
        }
        let output = cmd.output().unwrap();

        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    }
}
