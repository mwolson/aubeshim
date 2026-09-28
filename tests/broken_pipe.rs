use std::process::{Command, Output, Stdio};

// A pipe whose reader has already exited: the stdin of a finished child.
fn closed_pipe() -> Stdio {
    let mut reader = Command::new("sh")
        .args(["-c", "exit 0"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("spawn sh");
    let writer = reader.stdin.take().expect("child stdin");
    reader.wait().expect("wait for sh");
    Stdio::from(writer)
}

fn run_with_closed_stdout(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aubeshim"))
        .args(args)
        .stdout(closed_pipe())
        .stderr(Stdio::piped())
        .output()
        .expect("run aubeshim")
}

#[test]
fn activate_exits_quietly_when_reader_is_gone() {
    for shell in ["bash", "zsh", "fish", "sh"] {
        let output = run_with_closed_stdout(&["activate", shell]);
        assert!(
            output.status.success(),
            "activate {shell}: {:?}, stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty(), "activate {shell} wrote to stderr");
    }
}
