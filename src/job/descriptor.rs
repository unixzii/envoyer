use std::io;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::process::Stdio;
use std::sync::Arc;

use home::home_dir;
use nix::unistd::{self, Pid};
use tokio::io::AsyncWriteExt;
use tokio::net::unix::pipe;
use tokio::process::{Child as ChildProcess, Command};

#[derive(Clone, Debug)]
enum JobType {
    Script(Arc<str>),
}

struct SpawnContext {
    stdio_pipe: Option<pipe::Sender>,
}

#[derive(Clone, Debug)]
pub struct JobDescriptor {
    job_type: JobType,
}

impl JobDescriptor {
    pub fn script<S: AsRef<str>>(script: S) -> Self {
        let script_str = script.as_ref();
        Self {
            job_type: JobType::Script(Arc::from(script_str)),
        }
    }
}

impl JobDescriptor {
    /// Creates the process according to the descriptor.
    ///
    /// # Panics
    ///
    /// Panics if called from **outside** of the async context.
    pub(super) fn create_process(&self) -> io::Result<(ChildProcess, pipe::Receiver)> {
        let (pipe_tx, pipe_rx) = pipe::pipe()?;

        let mut cx = SpawnContext {
            stdio_pipe: Some(pipe_tx),
        };
        let child = match &self.job_type {
            JobType::Script(script) => Self::create_script_process(script, &mut cx),
        }?;

        Ok((child, pipe_rx))
    }

    fn decorate_command<'a>(
        cmd: &'a mut Command,
        cx: &mut SpawnContext,
    ) -> io::Result<&'a mut Command> {
        if let Some(home_dir) = home_dir() {
            cmd.current_dir(home_dir);
        } else {
            warn!("failed to get the home dir");
        }

        let pipe_raw_fd = cx
            .stdio_pipe
            .take()
            .expect("expected an opened pipe")
            .into_blocking_fd()?
            .into_raw_fd();
        // SAFETY: This redirects stdio to a valid file descriptor.
        unsafe {
            cmd.stdout(Stdio::from_raw_fd(pipe_raw_fd));
            cmd.stderr(Stdio::from_raw_fd(pipe_raw_fd));
        }

        // SAFETY: This is executed in the child process to set the
        // process group of it.
        unsafe {
            cmd.pre_exec(|| {
                unistd::setpgid(Pid::from_raw(0), Pid::from_raw(0))?;
                Ok(())
            });
        }

        Ok(cmd)
    }

    fn create_script_process(script: &Arc<str>, cx: &mut SpawnContext) -> io::Result<ChildProcess> {
        let mut cmd = Command::new("sh");
        let mut child = Self::decorate_command(&mut cmd, cx)?
            .stdin(Stdio::piped())
            .spawn()?;

        let mut stdin = child.stdin.take().expect("failed to open stdin");
        tokio::spawn({
            let script = Arc::clone(script);
            async move {
                if let Err(err) = stdin.write_all(script.as_bytes()).await {
                    error!("error occurred while piping script: {err:?}");
                }
            }
        });

        Ok(child)
    }
}
