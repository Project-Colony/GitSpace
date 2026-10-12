use std::process::Command;

#[test]
fn version_flag_prints_the_version_and_exits() {
    let output = Command::new(env!("CARGO_BIN_EXE_gitspace"))
        .arg("--version")
        .output()
        .expect("run gitspace --version");

    assert!(output.status.success(), "exit status: {}", output.status);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        concat!("gitspace ", env!("CARGO_PKG_VERSION"))
    );
}
