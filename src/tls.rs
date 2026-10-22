use std::{ffi::OsString, net::IpAddr, path::PathBuf};

use anyhow::{Context, bail};
use axum_server::tls_rustls::RustlsConfig;
use rcgen::{CertificateParams, KeyPair, PKCS_ED25519, SanType};

pub enum CertificateSource {
    Generated,
    Files { cert: PathBuf, key: PathBuf },
}

impl CertificateSource {
    pub fn from_paths(cert: Option<OsString>, key: Option<OsString>) -> anyhow::Result<Self> {
        match (cert, key) {
            (None, None) => Ok(Self::Generated),
            (Some(cert), Some(key)) if !cert.is_empty() && !key.is_empty() => Ok(Self::Files {
                cert: cert.into(),
                key: key.into(),
            }),
            _ => bail!("DROPZONE_CERT_PATH and DROPZONE_CERT_KEY_PATH must both be set to non-empty paths"),
        }
    }

    pub async fn config(&self, local_ip: Option<IpAddr>) -> anyhow::Result<RustlsConfig> {
        match self {
            Self::Generated => {
                let (cert, key) = generate_certificate(local_ip)?;
                RustlsConfig::from_pem(cert.into(), key.into())
                    .await
                    .context("could not configure generated TLS certificate")
            }
            Self::Files { cert, key } => RustlsConfig::from_pem_file(cert, key).await.with_context(|| {
                format!(
                    "could not load TLS certificate {} and private key {}",
                    cert.display(),
                    key.display()
                )
            }),
        }
    }
}

fn generate_certificate(local_ip: Option<IpAddr>) -> anyhow::Result<(String, String)> {
    let key = KeyPair::generate_for(&PKCS_ED25519).context("could not generate TLS private key")?;
    let cert = certificate_params(local_ip)?
        .self_signed(&key)
        .context("could not generate self-signed TLS certificate")?;
    Ok((cert.pem(), key.serialize_pem()))
}

fn certificate_params(local_ip: Option<IpAddr>) -> anyhow::Result<CertificateParams> {
    let mut params = CertificateParams::new(vec!["localhost".to_string()])?;
    for ip in [
        Some(IpAddr::from([127, 0, 0, 1])),
        Some(IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)),
        local_ip,
    ]
    .into_iter()
    .flatten()
    {
        let san = SanType::IpAddress(ip);
        if !params.subject_alt_names.contains(&san) {
            params.subject_alt_names.push(san);
        }
    }
    Ok(params)
}
