//! The CLI never panics on a stdout that stops reading (`emcraft-cli features | head`).

use std::io::Read;
use std::process::{Command, Stdio};

#[test]
fn a_closed_pipe_ends_the_command_quietly() {
    for args in [&["features"][..], &["commands"][..]] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_emcraft-cli")).args(args).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        // Read a few bytes, then close the pipe while the command still writes.
        let mut first = [0u8; 16];
        let mut stdout = child.stdout.take().unwrap();
        stdout.read_exact(&mut first).unwrap();
        drop(stdout);
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("panicked"), "{args:?}: {err}");
        assert!(out.status.success(), "{args:?}: {:?} {err}", out.status);
    }
}
