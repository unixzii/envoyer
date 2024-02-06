mod job;

use std::sync::Arc;

use axum::routing::{delete, get, post};

use crate::service::State;

type Router = axum::Router<Arc<State>>;

pub(super) fn mount_routes(router: Router) -> Router {
    router
        .route("/jobs", post(job::create))
        .route("/jobs/:job_id", delete(job::delete))
        .route("/jobs/:job_id/pid", get(job::get_pid))
}
