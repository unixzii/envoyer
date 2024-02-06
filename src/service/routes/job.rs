use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::job::JobDescriptor;
use crate::service::State as ServiceState;

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct DeleteParams {
    job_id: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct DeleteQueries {
    force: Option<bool>,
}

pub(super) async fn create(state: State<Arc<ServiceState>>, contents: Bytes) -> Body {
    let script = String::from_utf8(contents.to_vec()).expect("invalid script");

    let mut job = state
        .job_manager
        .create_job(&JobDescriptor::script(script))
        .await
        .unwrap();

    job.wait().await;

    Body::from(job.get_stdout().await)
}

pub(super) async fn delete(
    state: State<Arc<ServiceState>>,
    params: Path<DeleteParams>,
    queries: Query<DeleteQueries>,
) {
    let DeleteParams { job_id } = *params;
    let DeleteQueries { force } = *queries;

    let handle = state.job_manager.get_job(job_id).await.unwrap();
    if force.unwrap_or(false) {
        handle.kill().await;
    } else {
        handle.stop().await;
    }
}
