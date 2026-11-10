//! Root certificates for the server's HTTPS requests: torrent URLs and trackers.
//!
//! The clients check servers against the system's certificate store: the Keychain on macOS,
//! and on Linux the files OpenSSL reads (`SSL_CERT_FILE` or `SSL_CERT_DIR`, else the usual
//! places, Entware's among them). A router may have no store at all, and reqwest then refuses
//! to build a client, so the server didn't start. On Linux, when the system has no root
//! certificates, the clients use Mozilla's list instead, which the `webpki-root-certs` crate
//! compiles into the binary. A system that has a store is never given the built-in list.

/// What the server's HTTP clients are built from. A plain function, so the engine can take
/// it as `SessionOptions::http_client_builder`.
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "linux")]
    if !system_has_roots() {
        return builder.tls_certs_only(built_in_roots());
    }
    builder
}

/// Whether a client that trusts the system's certificates can be built: reqwest's verifier
/// refuses when it finds none it can use. Asked once, and the answer is the verifier's own.
#[cfg(target_os = "linux")]
fn system_has_roots() -> bool {
    static FOUND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FOUND.get_or_init(|| match reqwest::Client::builder().build() {
        Ok(_) => true,
        Err(error) => {
            // The whole chain, down to what the verifier said.
            tracing::info!(
                reason = %format!("{:#}", anyhow::Error::new(error)),
                "This system has no root certificates, so HTTPS uses the built-in list"
            );
            false
        }
    })
}

/// Mozilla's root certificates, as they were when this version was built.
#[cfg(any(target_os = "linux", test))]
fn built_in_roots() -> impl Iterator<Item = reqwest::Certificate> {
    webpki_root_certs::TLS_SERVER_ROOT_CERTS
        .iter()
        .filter_map(|der| reqwest::Certificate::from_der(der).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Runs on the Mac too, which never uses the list: the test build links it everywhere.
    #[test]
    fn a_client_can_be_built_from_the_built_in_roots() {
        let roots: Vec<_> = built_in_roots().collect();
        assert_eq!(roots.len(), webpki_root_certs::TLS_SERVER_ROOT_CERTS.len());
        assert!(roots.len() > 100, "{} root certificates", roots.len());
        // Every certificate must be one the TLS library accepts as a root, or this fails.
        reqwest::Client::builder()
            .tls_certs_only(roots)
            .build()
            .unwrap();
    }

    #[test]
    fn the_default_client_builds_here() {
        client_builder().build().unwrap();
    }
}
