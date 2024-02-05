use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use serde::{Deserialize, Serialize};

use crate::job::JobDescriptor;
use crate::service::State as ServiceState;

#[derive(Debug, Deserialize, Serialize)]
pub(super) struct DeleteParams {
    job_id: u64,
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

pub(super) async fn delete(state: State<Arc<ServiceState>>, params: Path<DeleteParams>) {
    let DeleteParams { job_id } = *params;

    let handle = state.job_manager.get_job(job_id).await.unwrap();
    handle.stop().await;
}
