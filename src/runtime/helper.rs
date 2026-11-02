use std::{
    os::windows::process::CommandExt,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use super::{RuntimeError, io_error};

pub(super) fn run_helper(command: &mut Command) -> Result<(), RuntimeError> {
    let helper = command.get_program().to_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .map_err(|source| io_error(&helper, source))?;
    wait_for_helper(&mut child, Path::new(&helper), Duration::from_secs(45))
}

fn wait_for_helper(
    child: &mut Child,
    helper: &Path,
    timeout: Duration,
) -> Result<(), RuntimeError> {
    let deadline = Instant::now() + timeout;
    let error = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(RuntimeError::HelperFailed(status.code().unwrap_or(-1)))
                };
            }
            Ok(None) => {}
            Err(source) => break io_error(helper, source),
        }
        if Instant::now() >= deadline {
            break RuntimeError::HelperTimeout;
        }
        thread::sleep(
            Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
        );
    };
    // Only the child created for the staged injection is owned by this operation.
    child.kill().map_err(|source| io_error(helper, source))?;
    child.wait().map_err(|source| io_error(helper, source))?;
    Err(error)
}

#[cfg(test)]
mod tests {
    use std::os::windows::process::CommandExt;
    use std::{
        env,
        path::PathBuf,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    use super::{run_helper, wait_for_helper};
    use crate::runtime::RuntimeError;

    #[test]
    fn helper_timeout_kills_and_reaps_only_the_owned_child() {
        let ping = PathBuf::from(env::var_os("SystemRoot").unwrap()).join("System32/ping.exe");
        let mut child = Command::new(&ping)
            .args(["127.0.0.1", "-n", "3"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let start = Instant::now();
        let result = wait_for_helper(&mut child, &ping, Duration::from_millis(30));
        assert!(
            matches!(result, Err(RuntimeError::HelperTimeout)),
            "{result:?}"
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn helper_success_and_nonzero_exit_are_reported() {
        let shell = env::var_os("COMSPEC").unwrap();
        assert!(run_helper(Command::new(&shell).args(["/D", "/C", "exit 0"])).is_ok());
        assert!(matches!(
            run_helper(Command::new(&shell).args(["/D", "/C", "exit 7"])),
            Err(RuntimeError::HelperFailed(7))
        ));
    }
}
