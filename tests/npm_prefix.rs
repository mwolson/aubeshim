#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

struct Sandbox {
    dir: tempfile::TempDir,
    npm_shim: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fake_tool(&bin.join("aube"), "aube");
        fake_tool(&bin.join("real-npm"), "real-npm");
        let npm_shim = bin.join("npm");
        symlink(env!("CARGO_BIN_EXE_aubeshim"), &npm_shim).unwrap();
        fs::create_dir_all(dir.path().join("home")).unwrap();
        Self { dir, npm_shim }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn npm(&self, cwd: &Path, args: &[&str]) -> String {
        let bin = self.path("bin");
        let output = Command::new(&self.npm_shim)
            .args(args)
            .current_dir(cwd)
            .env("AUBESHIM_AUBE", bin.join("aube"))
            .env("AUBESHIM_CONFIG", self.path("missing-config.toml"))
            .env("AUBESHIM_REAL_NPM", bin.join("real-npm"))
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("home/.config"))
            .output()
            .expect("run npm shim");
        assert!(
            output.status.success(),
            "npm {args:?}: {:?}, stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
}

fn fake_tool(path: &Path, name: &str) {
    fs::write(path, format!("#!/bin/sh\necho {name} \"$@\"\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn npm_install_into_bare_prefix_runs_real_npm() {
    let sandbox = Sandbox::new();
    let project = sandbox.path("project");
    let staging = project.join("tools/expo-device-hub/0.15.3");
    fs::create_dir_all(&staging).unwrap();
    fs::write(project.join("package.json"), "{}").unwrap();
    let staging = staging.to_string_lossy().into_owned();

    let stdout = sandbox.npm(
        &sandbox.path("home"),
        &[
            "install",
            "--prefix",
            &staging,
            "--no-fund",
            "--no-audit",
            "expo-device-hub@0.15.3",
        ],
    );

    assert_eq!(
        stdout,
        format!("real-npm install --prefix {staging} --no-fund --no-audit expo-device-hub@0.15.3")
    );
}

#[test]
fn npm_install_into_package_prefix_runs_aube() {
    let sandbox = Sandbox::new();
    let project = sandbox.path("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("package.json"), "{}").unwrap();
    let project = project.to_string_lossy().into_owned();

    let stdout = sandbox.npm(
        &sandbox.path("home"),
        &["install", "--prefix", &project, "--no-fund", "react"],
    );

    assert_eq!(stdout, format!("aube add --prefix {project} react"));
}
