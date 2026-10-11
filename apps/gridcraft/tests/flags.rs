//! Informational flags answer on stdout and exit without opening a window.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::{Command, Output};

/// Runs the desktop binary with no display, so a flag it doesn't recognize fails fast instead of
/// opening a window.
fn run(flag: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gridcraft")).arg(flag).env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY").output().expect("run gridcraft")
}

fn stdout_of(flag: &str) -> String {
    let o = run(flag);
    assert!(o.status.success(), "gridcraft {flag} failed: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn about_is_one_line_with_version_os_and_arch() {
    let out = stdout_of("--about");
    assert_eq!(out.lines().count(), 1, "{out:?}");
    assert!(out.contains(env!("CARGO_PKG_VERSION")), "{out:?}");
    assert!(out.contains(std::env::consts::OS), "{out:?}");
    assert!(out.contains(std::env::consts::ARCH), "{out:?}");
}

#[test]
fn help_lists_the_options() {
    for flag in ["--help", "-h"] {
        let out = stdout_of(flag);
        for option in ["--control", "--sample", "--about", "--version", "--help"] {
            assert!(out.contains(option), "{flag} lacks {option}: {out:?}");
        }
    }
}

#[test]
fn version_names_the_app() {
    assert_eq!(stdout_of("--version").trim(), format!("GridCraft {}", env!("CARGO_PKG_VERSION")));
}
