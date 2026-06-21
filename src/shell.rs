use clap::ValueEnum;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Fish,
    Sh,
    Zsh,
}

pub fn shell_init(shell: Shell, shim_dir: &Path, _persistent: bool) -> String {
    let dir = shell_quote(&shim_dir.to_string_lossy());
    match shell {
        Shell::Bash | Shell::Zsh => format!(
            "_aubeshim_shim_dir={dir}\nPATH=\":$PATH:\"\nPATH=\"${{PATH//:$_aubeshim_shim_dir:/:}}\"\nPATH=\"${{PATH#:}}\"\nPATH=\"${{PATH%:}}\"\nexport PATH=\"$_aubeshim_shim_dir:$PATH\"\nunset _aubeshim_shim_dir\n"
        ),
        Shell::Fish => format!(
            "set -l _aubeshim_shim_dir {dir}\nset -gx PATH (string match --invert -- $_aubeshim_shim_dir $PATH)\nfish_add_path --path --prepend $_aubeshim_shim_dir\nset -e _aubeshim_shim_dir\n"
        ),
        Shell::Sh => format!(
            "AUBESHIM_SHIM_DIR=${{AUBESHIM_SHIM_DIR:-{dir}}}\n_aubeshim_old_path=$PATH\nPATH=$AUBESHIM_SHIM_DIR\nIFS=:\nfor _aubeshim_path_entry in $_aubeshim_old_path; do\n    if [ \"$_aubeshim_path_entry\" != \"$AUBESHIM_SHIM_DIR\" ]; then\n        PATH=\"$PATH:$_aubeshim_path_entry\"\n    fi\ndone\nunset IFS _aubeshim_old_path _aubeshim_path_entry\nexport PATH\n"
        ),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
