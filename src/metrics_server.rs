#[cfg(feature = "metrics")]
use anyhow::Result;
#[cfg(feature = "metrics")]
use http_body_util::Full;
#[cfg(feature = "metrics")]
use hyper::body::Bytes;
#[cfg(feature = "metrics")]
use hyper::server::conn::http1;
#[cfg(feature = "metrics")]
use hyper::service::service_fn;
#[cfg(feature = "metrics")]
use hyper::{Request, Response, StatusCode};
#[cfg(feature = "metrics")]
use hyper_util::rt::TokioIo;
#[cfg(feature = "metrics")]
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
#[cfg(feature = "metrics")]
use std::net::SocketAddr;
#[cfg(feature = "metrics")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "metrics")]
use std::sync::Arc;
#[cfg(feature = "metrics")]
use tokio::net::TcpListener;
#[cfg(feature = "metrics")]
use tracing::{error, info};

#[cfg(feature = "metrics")]
pub fn install_recorder() -> Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new().install_recorder()?;
    Ok(handle)
}

#[cfg(feature = "metrics")]
pub async fn run_server(
    port: u16,
    handle: PrometheusHandle,
    health_flag: Arc<AtomicBool>,
) -> Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    info!("metrics and health server listening on {}", addr);

    loop {
        let (stream, _) = match listener.accept().await {
            Ok(conn) => conn,
            Err(err) => {
                error!("failed to accept connection: {}", err);
                continue;
            }
        };

        let handle_clone = handle.clone();
        let health_flag_clone = health_flag.clone();

        tokio::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |req: Request<hyper::body::Incoming>| {
                let handle = handle_clone.clone();
                let health = health_flag_clone.clone();
                async move { handle_request(req, handle, health).await }
            });

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                error!("error serving connection: {}", err);
            }
        });
    }
}

#[cfg(feature = "metrics")]
async fn handle_request(
    req: Request<hyper::body::Incoming>,
    handle: PrometheusHandle,
    health_flag: Arc<AtomicBool>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    match req.uri().path() {
        "/metrics" => {
            let metrics_text = handle.render();
            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4")
                .body(Full::new(Bytes::from(metrics_text)))
                .unwrap())
        }
        "/healthz" => {
            let is_healthy = health_flag.load(Ordering::Relaxed);
            if is_healthy {
                Ok(Response::builder()
                    .status(StatusCode::OK)
                    .body(Full::new(Bytes::from("OK")))
                    .unwrap())
            } else {
                Ok(Response::builder()
                    .status(StatusCode::SERVICE_UNAVAILABLE)
                    .body(Full::new(Bytes::from("Unhealthy")))
                    .unwrap())
            }
        }
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("Not Found")))
            .unwrap()),
    }
}
