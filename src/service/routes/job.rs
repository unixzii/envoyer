use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::job::{JobDescriptor, JobHandle};
use crate::service::error::{Error, Result};
use crate::service::State as ServiceState;

mod job_entry {
    use super::*;

    #[derive(Debug, Deserialize, Serialize)]
    pub struct JobEntry {
        job_id: u64,
        pid: Option<u32>,
        uptime: Option<u64>,
        started_at: String,
    }

    impl JobEntry {
        pub async fn from_job(job: &JobHandle) -> Self {
            let job_id = job.job_id();
            let pid = job.get_pid().await;
            let uptime = if pid.is_some() {
                Some(job.uptime().as_millis() as _)
            } else {
                None
            };
            let started_at = job.started_at_timestamp().format("%Y-%m-%dT%H:%M:%S.%3fZ");
            Self {
                job_id,
                pid,
                uptime,
                started_at: started_at.to_string(),
            }
        }
    }
}

use job_entry::JobEntry;

#[derive(Debug, Deserialize, Serialize)]
pub struct PathParams {
    job_id: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CreateResponse {
    job_id: u64,
}

pub async fn create(state: State<Arc<ServiceState>>, contents: Bytes) -> Result<CreateResponse> {
    let script = String::from_utf8(contents.to_vec()).map_err(|_| Error::unsupported_encoding())?;

    let job = state
        .job_manager
        .create_job(&JobDescriptor::script(script))
        .await
        .map_err(|err| Error::failed_to_spawn_with_details(format!("{err:?}")))?;

    Ok(CreateResponse {
        job_id: job.job_id(),
    })
    .into()
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ListResponse {
    jobs: Vec<JobEntry>,
}

pub async fn list(state: State<Arc<ServiceState>>) -> Result<ListResponse> {
    let jobs = state.job_manager.get_jobs().await;

    let mut job_entries = Vec::with_capacity(jobs.len());
    for job in jobs {
        job_entries.push(JobEntry::from_job(&job).await);
    }

    Ok(ListResponse { jobs: job_entries }).into()
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetResponse {
    #[serde(flatten)]
    job: JobEntry,
}

pub async fn get(state: State<Arc<ServiceState>>, params: Path<PathParams>) -> Result<GetResponse> {
    let PathParams { job_id } = *params;

    let handle = state
        .job_manager
        .get_job(job_id)
        .await
        .ok_or_else(Error::job_not_found)?;

    let job_entry = JobEntry::from_job(&handle).await;

    Ok(GetResponse { job: job_entry }).into()
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DeleteQueries {
    force: Option<bool>,
}

pub async fn delete(
    state: State<Arc<ServiceState>>,
    params: Path<PathParams>,
    queries: Query<DeleteQueries>,
) -> Result<()> {
    let PathParams { job_id } = *params;
    let DeleteQueries { force } = *queries;

    let handle = state
        .job_manager
        .get_job(job_id)
        .await
        .ok_or_else(Error::job_not_found)?;

    if force.unwrap_or(false) {
        handle.kill().await;
    } else {
        handle.stop().await;
    }

    Ok(()).into()
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GetPidResponse {
    pid: u32,
}

pub async fn get_pid(
    state: State<Arc<ServiceState>>,
    params: Path<PathParams>,
) -> Result<GetPidResponse> {
    let PathParams { job_id } = *params;

    let handle = state
        .job_manager
        .get_job(job_id)
        .await
        .ok_or_else(Error::job_not_found)?;

    let pid = handle.get_pid().await.ok_or_else(Error::job_has_finished)?;

    Ok(GetPidResponse { pid }).into()
}
