use std::collections::HashMap;
use std::io;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, ChildStdout};
use tokio::sync::RwLock;
use tokio::sync::{mpsc, watch};

use super::handle::Operation;
use super::{JobDescriptor, JobHandle, StdoutBuffer};

struct Inner {
    next_job_id: AtomicU64,
    child_handles: RwLock<HashMap<u64, JobHandle>>,
}

#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

impl JobManager {
    pub fn new() -> Self {
        let inner = Arc::new(Inner {
            next_job_id: AtomicU64::new(1),
            child_handles: Default::default(),
        });
        Self { inner }
    }

    pub async fn create_job(&self, desc: &JobDescriptor) -> io::Result<JobHandle> {
        debug!("creating job with descriptor: {desc:?}");

        let mut child = desc.create_process()?;
        debug!("process spawned, pid: {:?}", child.id());

        let job_id = self.inner.next_job_id.fetch_add(1, AtomicOrdering::Relaxed);

        let (operation_tx, operation_rx) = mpsc::channel(1);
        let (exit_status_tx, exit_status_rx) = watch::channel(None);

        let handle = JobHandle::new(operation_tx, exit_status_rx);

        if let Some(stdout) = child.stdout.take() {
            spawn_feeding_stdout(job_id, stdout, Arc::clone(&handle.stdout_buf));
        }

        spawn_polling_process(
            job_id,
            child,
            operation_rx,
            exit_status_tx,
            Arc::clone(&self.inner),
        );

        let mut child_handles = self.inner.child_handles.write().await;
        assert!(child_handles.insert(job_id, handle.clone()).is_none());

        Ok(handle)
    }

    pub async fn get_job(&self, job_id: u64) -> Option<JobHandle> {
        let child_handles = self.inner.child_handles.read().await;
        child_handles.get(&job_id).cloned()
    }
}

impl Default for JobManager {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

fn spawn_polling_process(
    job_id: u64,
    mut child: Child,
    mut operation_rx: mpsc::Receiver<Operation>,
    exit_status_tx: watch::Sender<Option<io::Result<ExitStatus>>>,
    mgr_inner: Arc<Inner>,
) {
    let fut = async move {
        let span = debug_span!("process_polling", job_id = ?job_id);

        debug!(parent: &span, "start polling, current pid: {:?}", child.id());
        loop {
            tokio::select! {
                exit_status = child.wait() => {
                    debug!(parent: &span, "process exited with status: {exit_status:?}");

                    // FIXME: Actually we should expect that the send will
                    // always succeed, since we hold at least one handle
                    // before the process exits.
                    _ = exit_status_tx.send(Some(exit_status));
                    break;
                }
                op = operation_rx.recv() => {
                    debug!(parent: &span, "received an operation");

                    let op = op.expect("operation channel closed too early");
                    op.perform_with_child(&mut child).await;
                }
            }
        }

        let mut child_handles = mgr_inner.child_handles.write().await;
        child_handles
            .remove(&job_id)
            .expect("internal state is inconsistent");
    };
    tokio::spawn(fut);
}

fn spawn_feeding_stdout(job_id: u64, mut stdout: ChildStdout, buf: Arc<StdoutBuffer>) {
    let fut = async move {
        let span = debug_span!("stdout_feeding", job_id = ?job_id);

        let mut stack_buf = [0u8; 512];
        debug!(parent: &span, "start feeding stdout");
        loop {
            let read_n = match stdout.read(&mut stack_buf).await {
                Ok(value) => value,
                Err(err) => {
                    if err.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }
                    error!(parent: &span, "error occurred: {err:?}");
                    return;
                }
            };

            if read_n == 0 {
                debug!(parent: &span, "reached eof");
                return;
            }

            trace!(parent: &span, "read {read_n} bytes");
            buf.write(&stack_buf[0..read_n]).await;
        }
    };
    tokio::spawn(fut);
}
