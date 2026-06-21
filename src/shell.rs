use clap::ValueEnum;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Fish,
    Sh,
    Zsh,
}

pub fn shell_init(shell: Shell, shim_dir: &Path, persistent: bool) -> String {
    let dir = shell_quote(&shim_dir.to_string_lossy());
    match shell {
        Shell::Bash => bash_init(&dir, persistent),
        Shell::Zsh => zsh_init(&dir, persistent),
        Shell::Fish => fish_init(&dir, persistent),
        Shell::Sh => sh_init(&dir),
    }
}

fn bash_init(dir: &str, persistent: bool) -> String {
    let mut out = bash_prepend_block(dir);
    if persistent {
        out.push_str(
            "if [[ \"${PROMPT_COMMAND:-}\" != *\"_aubeshim_prepend_path\"* ]]; then\n    PROMPT_COMMAND=\"_aubeshim_prepend_path${PROMPT_COMMAND:+; $PROMPT_COMMAND}\"\nfi\n",
        );
    }
    out
}

fn zsh_init(dir: &str, persistent: bool) -> String {
    let mut out = zsh_prepend_block(dir);
    if persistent {
        out.push_str(
            "autoload -Uz add-zsh-hook\nadd-zsh-hook -d precmd _aubeshim_prepend_path 2>/dev/null\nadd-zsh-hook precmd _aubeshim_prepend_path\n",
        );
    }
    out
}

fn fish_init(dir: &str, persistent: bool) -> String {
    let mut out = format!(
        "set -l _aubeshim_shim_dir {dir}\nset -gx PATH (string match --invert -- $_aubeshim_shim_dir $PATH)\nfish_add_path --path --prepend $_aubeshim_shim_dir\nset -e _aubeshim_shim_dir\n"
    );
    if persistent {
        out.push_str(&format!(
            "function _aubeshim_prepend_path --on-event fish_prompt\n    set -l _aubeshim_shim_dir {dir}\n    set -gx PATH (string match --invert -- $_aubeshim_shim_dir $PATH)\n    fish_add_path --path --prepend $_aubeshim_shim_dir\nend\n",
        ));
    }
    out
}

fn sh_init(dir: &str) -> String {
    format!(
        "AUBESHIM_SHIM_DIR=${{AUBESHIM_SHIM_DIR:-{dir}}}\n_aubeshim_old_path=$PATH\nPATH=$AUBESHIM_SHIM_DIR\nIFS=:\nfor _aubeshim_path_entry in $_aubeshim_old_path; do\n    if [ \"$_aubeshim_path_entry\" != \"$AUBESHIM_SHIM_DIR\" ]; then\n        PATH=\"$PATH:$_aubeshim_path_entry\"\n    fi\ndone\nunset IFS _aubeshim_old_path _aubeshim_path_entry\nexport PATH\n"
    )
}

fn bash_prepend_block(dir: &str) -> String {
    format!(
        "_aubeshim_shim_dir={dir}\n_aubeshim_prepend_path() {{\n    PATH=\":$PATH:\"\n    PATH=\"${{PATH//:$_aubeshim_shim_dir:/:}}\"\n    PATH=\"${{PATH#:}}\"\n    PATH=\"${{PATH%:}}\"\n    export PATH=\"$_aubeshim_shim_dir:$PATH\"\n}}\n_aubeshim_prepend_path\n"
    )
}

fn zsh_prepend_block(dir: &str) -> String {
    format!(
        "_aubeshim_shim_dir={dir}\n_aubeshim_prepend_path() {{\n    PATH=\":$PATH:\"\n    PATH=\"${{PATH//:$_aubeshim_shim_dir:/:}}\"\n    PATH=\"${{PATH#:}}\"\n    PATH=\"${{PATH%:}}\"\n    export PATH=\"$_aubeshim_shim_dir:$PATH\"\n}}\n_aubeshim_prepend_path\n"
    )
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
