mod builder;
mod error;
mod routes;
mod state;

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::http::Request;
use axum::{serve, Router};
use futures::future::BoxFuture;
use pin_project_lite::pin_project;
use thiserror::Error;
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::Span;

pub use builder::Builder;
use routes::RouterExt;
use state::State;

async fn make_tcp_listener() -> io::Result<(TcpListener, String)> {
    let addr = "0.0.0.0:8890";
    Ok((TcpListener::bind(addr).await?, addr.to_owned()))
}

async fn fallback_handler() -> error::Result<()> {
    error::Error::api_not_found().into()
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("unable to create TcpListener: {0:?}")]
    UnableToListen(io::Error),
}

pin_project! {
    pub struct Service {
        fut: BoxFuture<'static, io::Result<()>>,
    }
}

impl Service {
    async fn new(builder: Builder) -> Result<Self, Error> {
        let (listener, addr) = match make_tcp_listener().await {
            Ok(value) => value,
            Err(err) => return Err(Error::UnableToListen(err)),
        };

        let state = Arc::new(State {
            job_manager: builder.job_manager,
        });

        let router = Router::new()
            .mount_service_routes()
            .fallback(fallback_handler)
            .with_state(state)
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|req: &Request<_>| {
                        info_span!(
                            "http_request",
                            uri = ?req.uri(),
                            method = ?req.method()
                        )
                    })
                    .on_request(|_req: &Request<_>, span: &Span| {
                        info!(parent: span, "received a request");
                    }),
            );

        Ok(Self {
            fut: Box::pin(async move {
                info!("http server is listening at: {addr}");
                serve(listener, router).await
            }),
        })
    }
}

impl Future for Service {
    type Output = io::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let fut = this.fut.as_mut();
        fut.poll(cx)
    }
}
