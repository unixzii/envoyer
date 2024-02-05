use std::io;
use std::mem::MaybeUninit;
use std::process::ExitStatus;
use std::sync::Arc;

use nix::sys::signal;
use nix::unistd::Pid;
use tokio::process::Child;
use tokio::sync::{mpsc, oneshot, watch};

use super::StdoutBuffer;

pub(super) enum Operation {
    Stop,
    Kill,
    GetPid(oneshot::Sender<Option<u32>>),
}

impl Operation {
    pub async fn perform_with_child(self, child: &mut Child) {
        match self {
            Operation::Stop => {
                let Some(pid) = child.id() else {
                    warn!("process has exited");
                    return;
                };
                let pid = Pid::from_raw(pid as _);
                if let Err(err) = signal::killpg(pid, signal::SIGINT) {
                    error!("failed to send SIGINT to the process group: {err:?}");
                }
            }
            Operation::Kill => {
                if let Err(err) = child.start_kill() {
                    error!("failed to kill the process: {err:?}");
                }
            }
            Operation::GetPid(tx) => {
                let pid = child.id();
                // Don't care if the result is received.
                _ = tx.send(pid);
            }
        }
    }
}

#[derive(Clone)]
pub struct JobHandle {
    operation_tx: mpsc::Sender<Operation>,
    exit_status_rx: watch::Receiver<Option<io::Result<ExitStatus>>>,
    pub(super) stdout_buf: Arc<StdoutBuffer>,
}

impl JobHandle {
    #[inline]
    pub(super) fn new(
        operation_tx: mpsc::Sender<Operation>,
        exit_status_rx: watch::Receiver<Option<io::Result<ExitStatus>>>,
    ) -> Self {
        Self {
            operation_tx,
            exit_status_rx,
            stdout_buf: Default::default(),
        }
    }
}

impl JobHandle {
    /// Sends `SIGINT` signal to the process group of the job to
    /// simulate Ctrl-C action, and returns whether the request is
    /// successfully sent.
    pub async fn stop(&self) -> bool {
        self.operation_tx.send(Operation::Stop).await.is_ok()
    }

    /// Attempts to kill the process, and returns whether the request
    /// is successfully sent.
    pub async fn kill(&self) -> bool {
        self.operation_tx.send(Operation::Kill).await.is_ok()
    }

    pub async fn get_stdout(&self) -> Vec<u8> {
        self.stdout_buf.get_buffer().await
    }

    pub async fn wait(&mut self) {
        let mut result = MaybeUninit::uninit();
        self.exit_status_rx
            .wait_for(|value| {
                let Some(exit_status) = value else {
                    return false;
                };

                result.write(
                    exit_status
                        .as_ref()
                        .map(|es| *es)
                        .map_err(|err| io::Error::from(err.kind())),
                );

                true
            })
            .await
            .expect("channel dropped before sending the value");
    }
}
