use anyhow::Context;
use axum::response::IntoResponse;
use sha2::Digest;

/// Identifies the client that presented a pinned certificate on a given TLS connection.
#[derive(Clone, Debug)]
pub struct ClientIdentity {
    pub name: String,
    pub allow_webhook_access: bool,
}

/// Rejects any client whose certificate wasn't granted webhook access.
pub async fn require_webhook_access(
    axum::Extension(identity): axum::Extension<ClientIdentity>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if !identity.allow_webhook_access {
        return crate::controller::error::ControllerError::new(
            None,
            Some("Forbidden".to_string()),
            Some(format!("client '{}' is not authorized to access webhook routes", identity.name)),
            axum::http::StatusCode::FORBIDDEN,
        )
        .into_response();
    }

    next.run(req).await
}

/// Serve `app` over HTTPS.
///
/// If `client_auth` is [`crate::config::ClientAuth::Enabled`], every connection must present one
/// of the pinned client certificates or the handshake itself is rejected.
pub async fn serve(
    bind_addr: std::net::SocketAddr,
    cert_pem: String,
    key_pem: String,
    client_auth: crate::config::ClientAuth,
    app: axum::Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let cert_chain = parse_certs(&cert_pem)?;
    let key = parse_private_key(&key_pem)?;

    let (verifier, clients) = match client_auth {
        crate::config::ClientAuth::InsecureDisabled => (build_client_cert_verifier(None)?, None),
        crate::config::ClientAuth::Enabled { clients } => {
            let client_map = build_client_map(&clients)?;
            let verifier = build_client_cert_verifier(Some(&client_map))?;
            (verifier, Some(client_map))
        }
    };

    let mut server_config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(cert_chain, key)
        .context("invalid server certificate/key")?;
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(std::sync::Arc::new(server_config));
    let tls_acceptor = axum_server::tls_rustls::RustlsAcceptor::new(rustls_config);

    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    tokio::spawn(async move {
        shutdown.await;
        shutdown_handle.graceful_shutdown(None);
    });

    if let Some(clients) = clients {
        tracing::info!(bind = %bind_addr, mtls = true, "Starting API with TLS");
        axum_server::bind(bind_addr)
            .acceptor(MtlsAcceptor { inner: tls_acceptor, clients: std::sync::Arc::new(clients) })
            .handle(handle)
            .serve(app.into_make_service())
            .await
            .context("API server with mTLS failed")?;
    } else {
        tracing::info!(bind = %bind_addr, mtls = false, "Starting API with TLS");
        axum_server::bind(bind_addr)
            .acceptor(tls_acceptor)
            .handle(handle)
            .serve(app.into_make_service())
            .await
            .context("API server without mTLS failed")?;
    }

    Ok(())
}

fn parse_certs(pem: &str) -> anyhow::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let certs = rustls_pemfile::certs(&mut pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse PEM certificate(s)")?;

    if certs.is_empty() {
        anyhow::bail!("no certificates found in PEM input");
    }

    Ok(certs)
}

fn parse_private_key(pem: &str) -> anyhow::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    rustls_pemfile::private_key(&mut pem.as_bytes())
        .context("failed to parse PEM private key")?
        .ok_or_else(|| anyhow::anyhow!("no private key found in PEM input"))
}

fn fingerprint_hex(der: &[u8]) -> String {
    let digest = sha2::Sha256::digest(der);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn build_client_map(clients: &[crate::config::ClientCertConfig]) -> anyhow::Result<std::collections::HashMap<Vec<u8>, ClientIdentity>> {
    let mut map = std::collections::HashMap::new();

    for client in clients {
        let certs = parse_certs(&client.cert_pem).with_context(|| format!("invalid certificate for client '{}'", client.name))?;
        if certs.len() != 1 {
            anyhow::bail!("expected exactly one certificate for client '{}', found {}", client.name, certs.len());
        }
        let der = certs.into_iter().next().unwrap();

        tracing::info!(
            client = client.name,
            fingerprint = fingerprint_hex(der.as_ref()),
            "Pinned client certificate loaded"
        );

        map.insert(
            der.as_ref().to_vec(),
            ClientIdentity {
                name: client.name.clone(),
                allow_webhook_access: client.allow_webhook_access,
            },
        );
    }

    Ok(map)
}

/// Looks up the identity pinned to the given DER-encoded certificate bytes.
fn resolve_identity(der: &[u8], pins: &std::collections::HashMap<Vec<u8>, ClientIdentity>) -> Option<ClientIdentity> {
    pins.get(der).cloned()
}

/// Builds the client certificate verifier for the TLS handshake.
///
/// Note: pinned client certificates must be generated as non-CA leaf certificates,
/// or rustls-webpki rejects them when they're presented as the end-entity cert.
fn build_client_cert_verifier(
    clients: Option<&std::collections::HashMap<Vec<u8>, ClientIdentity>>,
) -> anyhow::Result<std::sync::Arc<dyn rustls::server::danger::ClientCertVerifier>> {
    let Some(clients) = clients else {
        return Ok(std::sync::Arc::new(rustls::server::NoClientAuth));
    };

    let mut roots = rustls::RootCertStore::empty();
    for der in clients.keys() {
        roots
            .add(rustls::pki_types::CertificateDer::from(der.clone()))
            .context("adding pinned client certificate as a trust anchor")?;
    }

    rustls::server::WebPkiClientVerifier::builder(std::sync::Arc::new(roots))
        .build()
        .context("building client certificate verifier")
}

/// Wraps [`axum_server::tls_rustls::RustlsAcceptor`] to extract the client identity established
/// during the mTLS handshake and attach it to every request on that connection via
/// [`tower_http::add_extension::AddExtension`].
#[derive(Clone)]
struct MtlsAcceptor {
    inner: axum_server::tls_rustls::RustlsAcceptor,
    clients: std::sync::Arc<std::collections::HashMap<Vec<u8>, ClientIdentity>>,
}

impl<I, S> axum_server::accept::Accept<I, S> for MtlsAcceptor
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Send + 'static,
{
    type Stream = tokio_rustls::server::TlsStream<I>;
    type Service = tower_http::add_extension::AddExtension<S, ClientIdentity>;
    type Future = std::pin::Pin<Box<dyn std::future::Future<Output = std::io::Result<(Self::Stream, Self::Service)>> + Send>>;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        let inner = self.inner.clone();
        let clients = self.clients.clone();

        Box::pin(async move {
            let (tls_stream, service) = inner.accept(stream, service).await?;

            let identity = {
                let (_, conn) = tls_stream.get_ref();
                conn.peer_certificates()
                    .and_then(|certs| certs.first())
                    .and_then(|cert| resolve_identity(cert.as_ref(), &clients))
            };

            let identity = match identity {
                Some(identity) => identity,
                // The verifier built by `build_client_cert_verifier` only completes the handshake for
                // a connection that presented one of the pinned certs.
                // This should not happen.
                None => return Err(std::io::Error::other("TLS connection completed without a recognized client certificate")),
            };

            Ok((tls_stream, tower_http::add_extension::AddExtension::new(service, identity)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_cert(common_name: &str) -> (String, String) {
        let rcgen::CertifiedKey { cert, signing_key } = rcgen::generate_simple_self_signed(vec![common_name.to_string()]).unwrap();
        (cert.pem(), signing_key.serialize_pem())
    }

    fn sample_pins() -> (std::collections::HashMap<Vec<u8>, ClientIdentity>, String, String) {
        let (cert_pem, key_pem) = generate_cert("known-client");
        let der = parse_certs(&cert_pem).unwrap().into_iter().next().unwrap();

        let mut pins = std::collections::HashMap::new();
        pins.insert(
            der.as_ref().to_vec(),
            ClientIdentity {
                name: "known".to_string(),
                allow_webhook_access: true,
            },
        );
        (pins, cert_pem, key_pem)
    }

    #[test]
    fn resolve_identity_matches_pinned_cert() {
        let (pins, cert_pem, _key_pem) = sample_pins();
        let der = parse_certs(&cert_pem).unwrap().into_iter().next().unwrap();

        let identity = resolve_identity(der.as_ref(), &pins).expect("known cert should resolve");
        assert_eq!(identity.name, "known");
        assert!(identity.allow_webhook_access);
    }

    #[test]
    fn resolve_identity_rejects_unpinned_cert() {
        let (pins, _cert_pem, _key_pem) = sample_pins();
        let (other_cert_pem, _) = generate_cert("unknown-client");
        let der = parse_certs(&other_cert_pem).unwrap().into_iter().next().unwrap();

        assert!(resolve_identity(der.as_ref(), &pins).is_none());
    }

    #[test]
    fn build_client_map_loads_configured_clients() {
        let (keycloak_cert_pem, _) = generate_cert("keycloak-client");
        let (monitoring_cert_pem, _) = generate_cert("monitoring-client");

        let clients = vec![
            crate::config::ClientCertConfig {
                name: "keycloak".to_string(),
                cert_pem: keycloak_cert_pem.clone(),
                allow_webhook_access: true,
            },
            crate::config::ClientCertConfig {
                name: "monitoring".to_string(),
                cert_pem: monitoring_cert_pem,
                allow_webhook_access: false,
            },
        ];

        let map = build_client_map(&clients).unwrap();
        assert_eq!(map.len(), 2);

        let keycloak_der = parse_certs(&keycloak_cert_pem).unwrap().into_iter().next().unwrap();
        let identity = resolve_identity(keycloak_der.as_ref(), &map).unwrap();
        assert_eq!(identity.name, "keycloak");
        assert!(identity.allow_webhook_access);
    }
}
