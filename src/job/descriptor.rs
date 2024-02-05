use std::io;
use std::process::Stdio;
use std::sync::Arc;

use home::home_dir;
use nix::unistd::{self, Pid};
use tokio::io::AsyncWriteExt;
use tokio::process::{Child as ChildProcess, Command};

#[derive(Clone, Debug)]
enum JobType {
    Script(Arc<str>),
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
    pub(super) fn create_process(&self) -> io::Result<ChildProcess> {
        match &self.job_type {
            JobType::Script(script) => self.create_script_process(script),
        }
    }

    fn decorate_command<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        if let Some(home_dir) = home_dir() {
            cmd.current_dir(home_dir);
        } else {
            warn!("failed to get the home dir");
        }
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        // SAFETY: This is executed in the child process to set the
        // process group of it.
        unsafe {
            cmd.pre_exec(|| {
                unistd::setpgid(Pid::from_raw(0), Pid::from_raw(0))?;
                Ok(())
            });
        }

        cmd
    }

    fn create_script_process(&self, script: &Arc<str>) -> io::Result<ChildProcess> {
        let mut cmd = Command::new("sh");
        let mut child = self
            .decorate_command(&mut cmd)
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
