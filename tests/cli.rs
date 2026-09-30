use std::process::Command;

#[test]
fn help_and_bad_arguments_work_without_display_or_credentials() {
    for args in [
        vec!["--help"],
        vec!["--calendar-until"],
        vec!["--calendar-until", "invalid"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_meeters"))
            .env_clear()
            .args(&args)
            .output()
            .unwrap();
        if args == ["--help"] {
            assert!(output.status.success());
            assert!(String::from_utf8(output.stdout)
                .unwrap()
                .contains("--calendar-until"));
            assert!(output.stderr.is_empty());
        } else {
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8(output.stderr)
                .unwrap()
                .contains("meeters:"));
        }
    }
}
