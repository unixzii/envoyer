#[macro_use]
extern crate tracing;

use std::process;

use anyhow::Result;
use envoyer::job::JobManager;
use envoyer::service::{self, Service};

fn init_logger() {
    use tracing_subscriber::{filter::LevelFilter, fmt, prelude::*, EnvFilter};
    let filter = EnvFilter::builder()
        .with_default_directive(
            if cfg!(debug_assertions) {
                LevelFilter::DEBUG
            } else {
                LevelFilter::INFO
            }
            .into(),
        )
        .from_env_lossy();
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();
}

fn init_job_manager() -> JobManager {
    JobManager::default()
}

async fn init_service(job_manager: JobManager) -> Result<Service> {
    debug!("initializing http server");
    let svc = service::Builder::with_job_manager(job_manager)
        .build()
        .await?;
    Ok(svc)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    init_logger();
    let job_manager = init_job_manager();
    let svc = match init_service(job_manager).await {
        Ok(value) => value,
        Err(err) => {
            error!("failed to initialize the http server: {err:?}");
            process::exit(1);
        }
    };

    if let Err(err) = svc.await {
        error!("http server is not terminated gracefully: {err:?}");
        process::exit(1);
    }
}
