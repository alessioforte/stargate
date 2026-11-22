//! Tests of process-wide state (settings read once, global providers, cached
//! configuration) run in a child process of the test binary.
//!
//! Each child starts in its own empty working directory. Files a process
//! creates under its working directory, such as the JWT keyring on first use,
//! are never shared with the parent or with other children.

use std::fs;
use std::process::{Command, Output};

/// Run `test` (its full path, matched with `--exact`) in a child process
/// configured by `configure`, and wait for it to exit.
pub(crate) fn run(test: &str, configure: impl FnOnce(&mut Command)) -> Output {
    let dir = std::env::temp_dir().join(format!("stargate-child-test-{}", ulid::Ulid::generate()));
    fs::create_dir(&dir).expect("create the child's working directory");
    let mut child = Command::new(std::env::current_exe().expect("test binary"));
    // The child runs the test its parent runs, ignored or not.
    child
        .args(["--exact", test, "--include-ignored", "--nocapture"])
        .current_dir(&dir);
    configure(&mut child);
    let output = child.output().expect("run the child test");
    fs::remove_dir_all(&dir).expect("remove the child's working directory");
    output
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn children_start_in_their_own_empty_working_directory() {
        const PARENT_DIR: &str = "STARGATE_TEST_PARENT_WORKING_DIRECTORY";
        let working_dir = std::env::current_dir().unwrap();
        if let Some(parent_dir) = std::env::var_os(PARENT_DIR) {
            assert_ne!(working_dir, Path::new(&parent_dir));
            assert_eq!(std::fs::read_dir(&working_dir).unwrap().count(), 0);
            println!("CHILD_WORKING_DIRECTORY={}", working_dir.display());
            return;
        }

        let output = super::run(
            "conf::child_test::tests::children_start_in_their_own_empty_working_directory",
            |command| {
                command.env(PARENT_DIR, &working_dir);
            },
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let child_dir = stdout
            .lines()
            .find_map(|line| line.strip_prefix("CHILD_WORKING_DIRECTORY="))
            .unwrap();
        assert!(
            !Path::new(child_dir).exists(),
            "removed after the child exits"
        );
    }
}
