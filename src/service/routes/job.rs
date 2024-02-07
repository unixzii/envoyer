use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::job::JobDescriptor;
use crate::service::error::{Error, Result};
use crate::service::State as ServiceState;

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
