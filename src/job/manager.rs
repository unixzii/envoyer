use std::collections::HashMap;
use std::io;
use std::mem::forget;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use tokio::io::AsyncReadExt;
use tokio::net::unix::pipe;
use tokio::process::Child;
use tokio::sync::RwLock;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;

use super::handle::Operation;
use super::{JobDescriptor, JobHandle, StdoutBuffer};

struct Inner {
    closed: RwLock<bool>,
    next_job_id: AtomicU64,
    job_handles: RwLock<HashMap<u64, JobHandle>>,
}

/// An job manager.
///
/// The manager provides a set of methods to manage jobs (e.g creating,
/// querying, stopping, etc.).
///
/// # Sharing
///
/// The manager object is designed to be shared. Cloning the object itself
/// will give you the same handle to the original manager.
///
/// `JobManager` is thread-safe, and by passing the object (or its clones)
/// into different tasks or threads, you will able to access the manager
/// from them.
///
/// # Shutdown
///
/// Shutting down should be explicit. When the last handle of a manager
/// instance dropped, the manager will shutdown only if there are no jobs
/// that are running.
///
/// After the manager is shutdown, other handles of the manager will not
/// be able to start new jobs.
///
/// To avoid leaking processes, it's recommended to shutdown the manager
/// explicitly before the program exits.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

impl JobManager {
    pub fn new() -> Self {
        let inner = Arc::new(Inner {
            closed: RwLock::new(false),
            next_job_id: AtomicU64::new(1),
            job_handles: Default::default(),
        });
        Self { inner }
    }

    pub async fn shutdown(&self) {
        let mut closed = self.inner.closed.write().await;
        if *closed {
            warn!("the job manager is already closed by other handles");
            return;
        }

        struct DropGuard<'a> {
            closed: &'a mut bool,
        }

        impl Drop for DropGuard<'_> {
            fn drop(&mut self) {
                *self.closed = true;
            }
        }

        let _drop_guard = DropGuard {
            closed: &mut closed,
        };

        let mut job_handles = self.inner.job_handles.write().await;
        for (_, mut job_handle) in job_handles.drain() {
            if job_handle.get_pid().await.is_none() {
                continue;
            }

            let job_id = job_handle.job_id();

            debug!("cleaning up job {job_id}");
            job_handle.stop().await;

            tokio::select! {
                _ = job_handle.wait() => { continue; }
                _ = sleep(Duration::from_secs(5)) => {
                    warn!(
                        "job {job_id} is not terminated in time, killing it with SIGKILL"
                    );
                    job_handle.kill().await;
                }
            }

            tokio::select! {
                _ = job_handle.wait() => { continue; }
                _ = sleep(Duration::from_secs(5)) => {
                    warn!("job {job_id} cannot be killed, it may be leaked");

                    // Dropping all handles of an unfinished job will make the
                    // polling task panic. We need to leak at least one handle
                    // to avoid such situation.
                    forget(job_handle);
                }
            }
        }
    }

    pub async fn create_job(&self, desc: &JobDescriptor) -> io::Result<JobHandle> {
        let closed = self.inner.closed.read().await;
        if *closed {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "the job manager is already closed",
            ));
        }

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

        drop(closed);
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
    job_handles.remove(&job_id);
}
