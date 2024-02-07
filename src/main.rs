#[macro_use]
extern crate tracing;

use std::process;

use anyhow::{Context, Result};
use envoyer::job::JobManager;
use envoyer::service::{self, Service};
use tokio::signal::unix::{signal, SignalKind};

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

async fn init_service() -> Result<Service> {
    let job_manager = JobManager::default();

    debug!("initializing http server");
    let svc = service::Builder::with_job_manager(job_manager)
        .build()
        .await?;
    Ok(svc)
}

async fn run_main_with_harness() -> Result<()> {
    let svc = init_service()
        .await
        .context("failed to initialize the http server")?;
    let job_manager = svc.job_manager().clone();

    let mut int_signal = signal(SignalKind::interrupt())?;

    let res = tokio::select! {
        _ = int_signal.recv() => {
            debug!("SIGINT received");
            Ok(())
        },
        res = svc => {
            res.context("http server is not terminated gracefully")
        }
    };

    info!("shutting down");
    job_manager.shutdown().await;

    res
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    init_logger();

    if let Err(err) = run_main_with_harness().await {
        error!("fatal error occurred: {err:?}");
        process::exit(1);
    }

    info!("bye");
}
