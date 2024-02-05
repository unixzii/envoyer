use super::{Error, Service};
use crate::job::JobManager;

#[derive(Clone)]
pub struct Builder {
    pub(super) job_manager: JobManager,
}

impl Builder {
    pub fn with_job_manager(job_manager: JobManager) -> Self {
        Self { job_manager }
    }

    pub async fn build(self) -> Result<Service, Error> {
        Service::new(self).await
    }
}
