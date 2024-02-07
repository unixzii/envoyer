use std::collections::HashMap;
use std::io;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use tokio::io::AsyncReadExt;
use tokio::net::unix::pipe;
use tokio::process::Child;
use tokio::sync::RwLock;
use tokio::sync::{mpsc, watch};

use super::handle::Operation;
use super::{JobDescriptor, JobHandle, StdoutBuffer};

struct Inner {
    next_job_id: AtomicU64,
    job_handles: RwLock<HashMap<u64, JobHandle>>,
}

#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

impl JobManager {
    pub fn new() -> Self {
        let inner = Arc::new(Inner {
            next_job_id: AtomicU64::new(1),
            job_handles: Default::default(),
        });
        Self { inner }
    }

    pub async fn create_job(&self, desc: &JobDescriptor) -> io::Result<JobHandle> {
        debug!("creating job with descriptor: {desc:?}");

        let (child, our_stdio_pipe) = desc.create_process()?;
        debug!("process spawned, pid: {:?}", child.id());

        let started_at = Instant::now();
        let started_at_ts = Utc::now();

        let job_id = self.inner.next_job_id.fetch_add(1, AtomicOrdering::Relaxed);
        let (operation_tx, operation_rx) = mpsc::channel(1);
        let (exit_status_tx, exit_status_rx) = watch::channel(None);
        let stdout_buf = Default::default();
        let handle = JobHandle::new(
            job_id,
            operation_tx,
            exit_status_rx,
            Arc::clone(&stdout_buf),
            started_at,
            started_at_ts,
        );

        let poll_fut = poll_process(
            job_id,
            child,
            our_stdio_pipe,
            stdout_buf,
            operation_rx,
            exit_status_tx,
            Arc::clone(&self.inner),
        );
        tokio::spawn(poll_fut);

        let mut job_handles = self.inner.job_handles.write().await;
        assert!(job_handles.insert(job_id, handle.clone()).is_none());

        Ok(handle)
    }

    pub async fn get_job(&self, job_id: u64) -> Option<JobHandle> {
        let job_handles = self.inner.job_handles.read().await;
        job_handles.get(&job_id).cloned()
    }

    pub async fn get_jobs(&self) -> Vec<JobHandle> {
        let job_handles = self.inner.job_handles.read().await;
        job_handles.values().cloned().collect()
    }
}

impl Default for JobManager {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

async fn poll_process(
    job_id: u64,
    mut child: Child,
    mut stdio_pipe: pipe::Receiver,
    buf: Arc<StdoutBuffer>,
    mut operation_rx: mpsc::Receiver<Operation>,
    exit_status_tx: watch::Sender<Option<io::Result<ExitStatus>>>,
    mgr_inner: Arc<Inner>,
) {
    let span = debug_span!("poll_process", job_id = ?job_id);

    let mut stack_buf = [0u8; 512];

    debug!(parent: &span, "start polling, current pid: {:?}", child.id());
    loop {
        tokio::select! {
            read_n = stdio_pipe.read(&mut stack_buf) => {
                match read_n {
                    Ok(read_n) => {
                        if read_n == 0 {
                            debug!(parent: &span, "reached eof");
                            break;
                        }

                        trace!(parent: &span, "read {read_n} bytes");
                        buf.write(&stack_buf[0..read_n]).await;
                    },
                    Err(err) => {
                        if err.kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        error!(parent: &span, "error occurred: {err:?}");
                        break;
                    },
                }
            }
            op = operation_rx.recv() => {
                debug!(parent: &span, "received an operation");

                let op = op.expect("operation channel closed too early");
                op.perform_with_child(&mut child).await;
            }
        }
    }

    // Stdio has been closed here, but we still need to wait for the process
    // to receive exit status and avoid zombie processes.
    loop {
        tokio::select! {
            exit_status = child.wait() => {
                debug!(parent: &span, "process exited with status: {exit_status:?}");

                // We expect that the receiver will never get dropped before
                // it receives the exit status, since we hold at least one
                // handle until the process is cleaned up.
                exit_status_tx.send(Some(exit_status)).expect("channel closed too early");
                break;
            }
            op = operation_rx.recv() => {
                debug!(parent: &span, "received an operation");

                let op = op.expect("operation channel closed too early");
                op.perform_with_child(&mut child).await;
            }
        }
    }

    let mut job_handles = mgr_inner.job_handles.write().await;
    job_handles
        .remove(&job_id)
        .expect("internal state is inconsistent");
}
