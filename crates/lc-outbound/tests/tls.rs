//! TLS and password auth against the `cassandra-tls` fixture.
//!
//! Run with `mise run test:it` after `mise run db:tls:up`.

#![cfg(feature = "integration")]

use std::path::PathBuf;

use lc_domain::config::{
    ConnectionConfig, ConnectionName, Credential, Credentials, Discovery, DriverConfig, Tls,
    TlsSettings, Trust, Verification,
};
use lc_domain::model::{ConnId, DbError};
use lc_domain::ports::Cluster;
use lc_domain::posture::Posture;
use lc_outbound::cassandra::CassandraCluster;

fn certs(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docker/tls/certs")
        .join(file)
}

fn login(password: &str) -> Credentials {
    Credentials::Stated {
        username: "cassandra".to_owned(),
        password: Credential::Literal(password.to_owned()),
    }
}

#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn target(tls: Tls, credentials: Option<Credentials>) -> ConnectionConfig {
    let port = std::env::var("LC_CASSANDRA_TLS_PORT").unwrap_or_else(|_| "9142".to_owned());

    ConnectionConfig {
        name: ConnectionName::try_new("tls").expect("a valid name"),
        hosts: vec![format!("127.0.0.1:{port}")],
        posture: Posture::ReadOnly,
        discovery: Discovery::default(),
        datacenter: None,
        credentials,
        tls,
    }
}

fn trusting(ca: &str, verification: Verification) -> Tls {
    Tls::On(TlsSettings {
        trust: Trust::CaFile(certs(ca)),
        identity: None,
        verification,
    })
}

async fn connect(target: &ConnectionConfig) -> Result<(), DbError> {
    CassandraCluster::new()
        .connect(ConnId::next(), target, &DriverConfig::default())
        .await
        .map(|_| ())
}

#[tokio::test]
async fn a_cluster_whose_ca_is_trusted_and_whose_login_is_right_connects() {
    let tls = trusting("ca.pem", Verification::Full);

    connect(&target(tls, Some(login("cassandra"))))
        .await
        .expect("TLS and the login both hold");
}

#[tokio::test]
async fn a_wrong_password_is_an_authentication_failure() {
    let tls = trusting("ca.pem", Verification::Full);

    let error = connect(&target(tls, Some(login("nope"))))
        .await
        .expect_err("wrong password");

    assert!(matches!(error, DbError::Authentication(_)), "{error:?}");
}

#[tokio::test]
async fn no_login_against_a_password_cluster_is_an_authentication_failure() {
    let tls = trusting("ca.pem", Verification::Full);

    let error = connect(&target(tls, None)).await.expect_err("no login");

    assert!(matches!(error, DbError::Authentication(_)), "{error:?}");
}

#[tokio::test]
async fn an_issuer_the_client_does_not_trust_is_a_tls_failure() {
    let tls = trusting("other-ca.pem", Verification::Full);

    let error = connect(&target(tls, Some(login("cassandra"))))
        .await
        .expect_err("unknown issuer");

    assert!(matches!(error, DbError::Tls(_)), "{error:?}");
}

#[tokio::test]
async fn a_name_the_certificate_does_not_carry_fails_unless_the_chain_is_all_that_is_checked() {
    let wrong = trusting("ca.pem", Verification::Named("other.test".to_owned()));
    let error = connect(&target(wrong, Some(login("cassandra"))))
        .await
        .expect_err("wrong name");
    assert!(matches!(error, DbError::Tls(_)), "{error:?}");

    let right = trusting("ca.pem", Verification::Named("cassandra.test".to_owned()));
    connect(&target(right, Some(login("cassandra"))))
        .await
        .expect("the certificate names it");

    let chain = trusting("ca.pem", Verification::ChainOnly);
    connect(&target(chain, Some(login("cassandra"))))
        .await
        .expect("the name is not checked");
}

#[tokio::test]
async fn verification_off_accepts_an_issuer_nobody_trusts() {
    let tls = trusting("other-ca.pem", Verification::Off);

    connect(&target(tls, Some(login("cassandra"))))
        .await
        .expect("nothing is checked");
}

#[tokio::test]
async fn plaintext_against_a_tls_cluster_does_not_connect() {
    let error = connect(&target(Tls::Off, Some(login("cassandra"))))
        .await
        .expect_err("TLS required");

    assert!(
        matches!(&error, DbError::Unreachable(m) if m.contains("tls")),
        "{error:?}"
    );
}

#[allow(
    clippy::expect_used,
    reason = "allow-expect-in-tests only covers #[test]-attributed items, not helpers they call"
)]
fn cqlshrc(validate: bool) -> Credentials {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cqlsh-tls");
    std::fs::create_dir_all(&dir).expect("a scratch directory");

    let credentials = dir.join("credentials");
    std::fs::write(
        &credentials,
        "[PlainTextAuthProvider]\npassword = cassandra\n",
    )
    .expect("the credentials are written");

    let rc = dir.join("cqlshrc");
    std::fs::write(
        &rc,
        format!(
            "[auth_provider]\nusername = cassandra\n\n[authentication]\ncredentials = {}\n\n\
             [connection]\nssl = true\n\n[ssl]\ncertfile = {}\nvalidate = {validate}\n",
            credentials.display(),
            certs("ca.pem").display(),
        ),
    )
    .expect("the cqlshrc is written");

    Credentials::Cqlshrc(rc)
}

#[tokio::test]
async fn a_cqlshrc_supplies_both_the_login_and_the_tls_settings() {
    let Credentials::Cqlshrc(rc) = cqlshrc(true) else {
        unreachable!("cqlshrc builds a Cqlshrc");
    };

    connect(&target(
        Tls::Cqlshrc(rc.clone()),
        Some(Credentials::Cqlshrc(rc)),
    ))
    .await
    .expect("the cqlshrc alone is enough");
}
