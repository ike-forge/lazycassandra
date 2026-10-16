//! `TlsSettings` into the `rustls::ClientConfig` the driver dials with.

use std::path::Path;
use std::sync::Arc;

use lc_domain::config::{ClientIdentity, TlsSettings, Trust, Verification};
use lc_domain::model::DbError;
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, aws_lc_rs};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme,
};

use crate::secret::expand_home;

/// # Errors
///
/// [`DbError::Tls`] when a stated file is unreadable or holds no usable PEM, or
/// there are no trusted roots.
pub(crate) async fn client_config(settings: &TlsSettings) -> Result<Arc<ClientConfig>, DbError> {
    let provider = Arc::new(aws_lc_rs::default_provider());
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(tls)?;

    let builder = if settings.verification == Verification::Off {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Unverified {
                algorithms: provider.signature_verification_algorithms,
            }))
    } else {
        let roots = Arc::new(roots(&settings.trust).await?);
        let inner = WebPkiServerVerifier::builder_with_provider(roots, provider)
            .build()
            .map_err(tls)?;

        match &settings.verification {
            Verification::Full | Verification::Off => {
                builder.dangerous().with_custom_certificate_verifier(inner)
            }
            Verification::Named(name) => {
                let name = ServerName::try_from(name.clone())
                    .map_err(|error| DbError::Tls(format!("server_name `{name}`: {error}")))?;

                builder
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(Relaxed {
                        inner,
                        name: Some(name),
                    }))
            }
            Verification::ChainOnly => builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(Relaxed { inner, name: None })),
        }
    };

    let config = match &settings.identity {
        None => builder.with_no_client_auth(),
        Some(identity) => {
            let (chain, key) = identity_pair(identity).await?;

            builder.with_client_auth_cert(chain, key).map_err(tls)?
        }
    };

    Ok(Arc::new(config))
}

fn tls(error: impl std::fmt::Display) -> DbError {
    DbError::Tls(error.to_string())
}

async fn read(path: &Path) -> Result<Vec<u8>, DbError> {
    let path = expand_home(path);

    tokio::fs::read(&path)
        .await
        .map_err(|error| DbError::Tls(format!("{}: {error}", path.display())))
}

async fn roots(trust: &Trust) -> Result<RootCertStore, DbError> {
    let mut store = RootCertStore::empty();

    match trust {
        Trust::System => {
            // Reads the filesystem, or off Linux the keychain.
            let loaded = tokio::task::spawn_blocking(rustls_native_certs::load_native_certs)
                .await
                .map_err(tls)?;
            let (added, _) = store.add_parsable_certificates(loaded.certs);

            if added == 0 {
                let why = loaded
                    .errors
                    .first()
                    .map_or_else(|| "none found".to_owned(), ToString::to_string);

                return Err(DbError::Tls(format!(
                    "no system root certificates ({why}); state `ca` to trust a file"
                )));
            }
        }
        Trust::CaFile(path) => {
            let pem = read(path).await?;

            for cert in CertificateDer::pem_slice_iter(&pem) {
                store.add(cert.map_err(tls)?).map_err(tls)?;
            }

            if store.is_empty() {
                return Err(DbError::Tls(format!(
                    "{}: no certificates",
                    expand_home(path).display()
                )));
            }
        }
    }

    Ok(store)
}

async fn identity_pair(
    identity: &ClientIdentity,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), DbError> {
    let chain = CertificateDer::pem_slice_iter(&read(&identity.cert).await?)
        .collect::<Result<Vec<_>, _>>()
        .map_err(tls)?;

    if chain.is_empty() {
        return Err(DbError::Tls(format!(
            "{}: no certificates",
            expand_home(&identity.cert).display()
        )));
    }

    let key = PrivateKeyDer::from_pem_slice(&read(&identity.key).await?).map_err(tls)?;

    Ok((chain, key))
}

/// webpki checks the name after the chain, so a name error means the chain
/// passed.
#[derive(Debug)]
struct Relaxed {
    inner: Arc<WebPkiServerVerifier>,
    /// `None` skips the name check.
    name: Option<ServerName<'static>>,
}

impl ServerCertVerifier for Relaxed {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let name = self.name.as_ref().unwrap_or(server_name);
        let verdict =
            self.inner
                .verify_server_cert(end_entity, intermediates, name, ocsp_response, now);

        match verdict {
            Err(Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) if self.name.is_none() => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// The handshake signature is still checked.
#[derive(Debug)]
struct Unverified {
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for Unverified {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_missing_ca_file_names_the_file() {
        let settings = TlsSettings {
            trust: Trust::CaFile("/nonexistent/ca.pem".into()),
            ..TlsSettings::default()
        };

        let error = client_config(&settings).await.expect_err("no such file");

        assert!(matches!(&error, DbError::Tls(m) if m.contains("/nonexistent/ca.pem")));
    }

    #[tokio::test]
    async fn an_unverified_config_needs_no_roots() {
        let settings = TlsSettings {
            trust: Trust::CaFile("/nonexistent/ca.pem".into()),
            verification: Verification::Off,
            ..TlsSettings::default()
        };

        assert!(client_config(&settings).await.is_ok());
    }
}
