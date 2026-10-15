// Each integration test binary uses a different subset of these helpers.
#![allow(dead_code)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub struct Sandbox {
    dir: tempfile::TempDir,
}

impl Sandbox {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fake_tool(&bin.join("aube"), "aube");
        fake_tool(&bin.join("real-npm"), "real-npm");
        symlink(env!("CARGO_BIN_EXE_aubeshim"), bin.join("npm")).unwrap();
        fs::create_dir_all(dir.path().join("home")).unwrap();
        Self { dir }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    pub fn npm_output(&self, cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Output {
        let bin = self.path("bin");
        let output = Command::new(bin.join("npm"))
            .args(args)
            .current_dir(cwd)
            .env("AUBESHIM_AUBE", bin.join("aube"))
            .env("AUBESHIM_CONFIG", self.path("missing-config.toml"))
            .env("AUBESHIM_REAL_NPM", bin.join("real-npm"))
            .env("HOME", self.path("home"))
            .env("XDG_CONFIG_HOME", self.path("home/.config"))
            .env_remove("AUBESHIM_DEBUG")
            .env_remove("AUBE_MINIMUM_PACKAGE_AGE")
            .env_remove("AUBE_NODE_LINKER")
            .env_remove("AUBE_PACKAGE_IMPORT_METHOD")
            .envs(envs.iter().copied())
            .output()
            .expect("run npm shim");
        assert!(
            output.status.success(),
            "npm {args:?}: {:?}, stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    pub fn npm(&self, cwd: &Path, args: &[&str]) -> String {
        let output = self.npm_output(cwd, args, &[]);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
}

fn fake_tool(path: &Path, name: &str) {
    fs::write(path, format!("#!/bin/sh\necho {name} \"$@\"\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
