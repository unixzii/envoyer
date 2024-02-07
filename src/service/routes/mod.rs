mod job;

use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;

use crate::service::State;

pub trait RouterExt {
    fn mount_service_routes(self) -> Router<Arc<State>>;
}

impl RouterExt for Router<Arc<State>> {
    fn mount_service_routes(self) -> Router<Arc<State>> {
        self.route("/jobs", post(job::create))
            .route("/jobs", get(job::list))
            .route("/jobs/:job_id", get(job::get))
            .route("/jobs/:job_id", delete(job::delete))
            .route("/jobs/:job_id/pid", get(job::get_pid))
    }
}
