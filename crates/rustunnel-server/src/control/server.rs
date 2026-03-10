//! Control-plane TCP listener.
//!
//! Accepts TCP connections, upgrades to TLS (tokio-rustls), then upgrades to
//! WebSocket (tokio-tungstenite), and spawns a session handler for each.

use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::accept_async;

use rustls::ServerConfig as RustlsConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls_pemfile::{certs, private_key};

use crate::config::ServerConfig;
use crate::core::TunnelCore;
use crate::error::{Error, Result};
use crate::control::session::handle_session;

/// Start the control-plane listener.
///
/// Binds `addr`, wraps accepted sockets in TLS, upgrades them to WebSocket,
/// and spawns a `handle_session` task for each connection.
pub async fn run_control_plane(
    addr:   SocketAddr,
    core:   Arc<TunnelCore>,
    config: Arc<ServerConfig>,
) -> Result<()> {
    let tls_acceptor = build_tls_acceptor(&config)?;
    let listener    = TcpListener::bind(addr).await?;

    tracing::info!(%addr, "control plane listening");

    loop {
        let (tcp_stream, peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("accept error: {e}");
                continue;
            }
        };

        tracing::debug!(%peer_addr, "new TCP connection");

        let acceptor = tls_acceptor.clone();
        let core     = core.clone();
        let config   = config.clone();

        tokio::spawn(async move {
            // TLS handshake.
            let tls_stream = match acceptor.accept(tcp_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(%peer_addr, "TLS handshake failed: {e}");
                    return;
                }
            };
            tracing::debug!(%peer_addr, "TLS handshake complete");

            // WebSocket upgrade.
            let ws_stream = match accept_async(tls_stream).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(%peer_addr, "WebSocket upgrade failed: {e}");
                    return;
                }
            };
            tracing::debug!(%peer_addr, "WebSocket upgrade complete");

            handle_session(ws_stream, peer_addr, core, config).await;
        });
    }
}

// ── TLS setup ─────────────────────────────────────────────────────────────────

fn build_tls_acceptor(config: &ServerConfig) -> Result<TlsAcceptor> {
    let certs = load_certs(&config.tls.cert_path)?;
    let key   = load_private_key(&config.tls.key_path)?;

    let rustls_config = RustlsConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| Error::Config(format!("TLS config error: {e}")))?;

    Ok(TlsAcceptor::from(Arc::new(rustls_config)))
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let f = File::open(path)
        .map_err(|e| Error::Config(format!("cannot open cert file {path}: {e}")))?;
    certs(&mut BufReader::new(f))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| Error::Config(format!("invalid cert PEM {path}: {e}")))
}

fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let f = File::open(path)
        .map_err(|e| Error::Config(format!("cannot open key file {path}: {e}")))?;
    private_key(&mut BufReader::new(f))
        .map_err(|e| Error::Config(format!("invalid key PEM {path}: {e}")))?
        .ok_or_else(|| Error::Config(format!("no private key found in {path}")))
}
