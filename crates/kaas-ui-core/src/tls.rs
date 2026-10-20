//! The certificate each broker presented, read off kaas-ui's own connection.
//!
//! ## Per broker, because that is all there is
//!
//! Kafka has no api that returns a certificate. The only one kaas-ui can know
//! is the one a broker presented in the handshake of *our* connection to it,
//! which kaas-lib keeps on the connection as [`TlsSession`]. So, like the
//! version table, there is no cluster certificate to show: every broker has
//! its own, brokers mid-rotation disagree, and that disagreement is the thing
//! worth seeing. A cluster summary is the UI's job, folded from these rows.
//!
//! Two limits, both stated in the UI rather than discovered:
//!
//! * It is the listener *kaas-ui* dials. Strimzi serves three, each of which
//!   may present a different certificate, and this sees one of them.
//! * A chain that failed verification produced no connection, so it has no
//!   row here — it is the broker's error instead, and rustls' message names
//!   what it rejected (the names presented, or the expiry).
//!
//! ## Parsing lives here, not in kaas-lib
//!
//! kaas-lib hands over DER and declines to parse it, for the reason
//! [`TlsSession`] gives. The fields below are the ones that decide whether a
//! Kafka client can connect: SANs (every broker's advertised name must be
//! covered, not only the bootstrap's), validity, issuer, key usage, and the
//! key and signature algorithms. A field this parser cannot read is absent
//! rather than guessed.

use kafka_conn::TlsSession;
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use utoipa::ToSchema;
use x509_cert::Certificate;
use x509_cert::der::asn1::{ObjectIdentifier, UintRef};
use x509_cert::der::oid::db::{DB, rfc5280, rfc5912};
use x509_cert::der::{Decode as _, Reader as _, SliceReader};
use x509_cert::ext::pkix::name::GeneralName;
use x509_cert::ext::pkix::{BasicConstraints, ExtendedKeyUsage, KeyUsage, SubjectAltName};

/// One broker's TLS session, as kaas-ui's connection to it negotiated.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BrokerTls {
    /// Node id.
    pub node_id: i32,
    /// The address dialled — the advertised listener.
    pub peer: String,
    /// `None` on a plaintext listener. Not an error: absence of TLS is a
    /// configuration, and the UI says so rather than warning.
    pub session: Option<TlsSessionDto>,
}

/// What one handshake agreed on, and the chain that came with it.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TlsSessionDto {
    /// `TLSv1.2` or `TLSv1.3`.
    pub protocol_version: String,
    /// The cipher suite, by IANA name.
    pub cipher_suite: String,
    /// The name the leaf was verified against. The advertised host unless a
    /// server-name override is configured — compare SANs against this.
    pub server_name: String,
    /// The chain as the broker sent it, leaf first.
    pub chain: Vec<CertificateDto>,
}

/// The fields of one certificate that decide whether a Kafka client connects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CertificateDto {
    /// Subject distinguished name, RFC 4514.
    pub subject: String,
    /// Issuer distinguished name, RFC 4514.
    pub issuer: String,
    /// Serial number, colon-separated hex.
    pub serial: String,
    /// Start of validity, milliseconds since the epoch.
    pub not_before: i64,
    /// End of validity, milliseconds since the epoch.
    pub not_after: i64,
    /// DNS subject alternative names. Hostname verification reads these and
    /// nothing else — the subject CN is ignored when they are present.
    pub dns_names: Vec<String>,
    /// IP-address subject alternative names.
    pub ip_addresses: Vec<String>,
    /// Key usage bits, by RFC 5280 name. Empty when the extension is absent.
    pub key_usage: Vec<String>,
    /// Extended key usage purposes. A broker's leaf needs `serverAuth`.
    pub extended_key_usage: Vec<String>,
    /// From basicConstraints. `None` when the extension is absent, which is
    /// not the same claim as `false`.
    pub is_ca: Option<bool>,
    /// Key algorithm and size, e.g. `RSA 2048` or `EC secp256r1`.
    pub public_key: String,
    /// Signature algorithm, e.g. `sha256WithRSAEncryption`.
    pub signature_algorithm: String,
    /// Subject equals issuer.
    pub self_signed: bool,
    /// SHA-256 of the DER, colon-separated hex — what `openssl x509
    /// -fingerprint -sha256` prints, so the two can be compared by eye.
    pub sha256_fingerprint: String,
}

impl TlsSessionDto {
    /// Parse a session's chain.
    ///
    /// All or nothing: rustls already parsed every certificate here to verify
    /// it, so one this parser cannot read is a disagreement between two
    /// parsers, and showing a partial chain would hide it.
    pub fn from_session(session: &TlsSession) -> Result<Self, String> {
        let chain = session
            .peer_certificates()
            .iter()
            .enumerate()
            .map(|(index, der)| {
                CertificateDto::from_der(der)
                    .map_err(|error| format!("certificate {index} of the chain: {error}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            protocol_version: session.protocol_version().to_owned(),
            cipher_suite: session.cipher_suite().to_owned(),
            server_name: session.server_name().to_owned(),
            chain,
        })
    }
}

impl CertificateDto {
    /// Parse one DER-encoded certificate.
    pub fn from_der(der: &[u8]) -> Result<Self, String> {
        let cert =
            Certificate::from_der(der).map_err(|e| format!("not an X.509 certificate: {e}"))?;
        let tbs = &cert.tbs_certificate;

        let mut dns_names = Vec::new();
        let mut ip_addresses = Vec::new();
        if let Some((_, SubjectAltName(names))) = extension::<SubjectAltName>(&cert)? {
            for name in names {
                match name {
                    GeneralName::DnsName(dns) => dns_names.push(dns.to_string()),
                    GeneralName::IpAddress(ip) => ip_addresses.push(ip_address(ip.as_bytes())),
                    _ => {}
                }
            }
        }

        let key_usage = extension::<KeyUsage>(&cert)?
            .map(|(_, usage)| key_usage_names(&usage))
            .unwrap_or_default();
        let extended_key_usage = extension::<ExtendedKeyUsage>(&cert)?
            .map(|(_, ExtendedKeyUsage(purposes))| purposes.iter().map(purpose_name).collect())
            .unwrap_or_default();
        let is_ca = extension::<BasicConstraints>(&cert)?.map(|(_, constraints)| constraints.ca);

        Ok(Self {
            subject: tbs.subject.to_string(),
            issuer: tbs.issuer.to_string(),
            serial: colon_hex(tbs.serial_number.as_bytes()),
            not_before: millis(tbs.validity.not_before.to_unix_duration()),
            not_after: millis(tbs.validity.not_after.to_unix_duration()),
            dns_names,
            ip_addresses,
            key_usage,
            extended_key_usage,
            is_ca,
            public_key: public_key(&tbs.subject_public_key_info),
            signature_algorithm: oid_name(&cert.signature_algorithm.oid),
            self_signed: tbs.subject == tbs.issuer,
            sha256_fingerprint: colon_hex(&Sha256::digest(der)),
        })
    }
}

/// One extension, decoded. A present-but-malformed extension is an error, not
/// an absence: rendering a broken SAN list as "no SANs" would be a lie.
fn extension<'a, T>(cert: &'a Certificate) -> Result<Option<(bool, T)>, String>
where
    T: x509_cert::der::Decode<'a> + x509_cert::der::oid::AssociatedOid,
{
    cert.tbs_certificate
        .get::<T>()
        .map_err(|e| format!("extension {}: {e}", oid_name(&T::OID)))
}

fn key_usage_names(usage: &KeyUsage) -> Vec<String> {
    [
        (usage.digital_signature(), "digitalSignature"),
        (usage.non_repudiation(), "nonRepudiation"),
        (usage.key_encipherment(), "keyEncipherment"),
        (usage.data_encipherment(), "dataEncipherment"),
        (usage.key_agreement(), "keyAgreement"),
        (usage.key_cert_sign(), "keyCertSign"),
        (usage.crl_sign(), "cRLSign"),
        (usage.encipher_only(), "encipherOnly"),
        (usage.decipher_only(), "decipherOnly"),
    ]
    .into_iter()
    .filter(|(set, _)| *set)
    .map(|(_, name)| name.to_owned())
    .collect()
}

/// `serverAuth` rather than `id-kp-serverAuth`: the short name is what
/// `openssl x509 -text` prints, and what an operator will search for.
fn purpose_name(oid: &ObjectIdentifier) -> String {
    match *oid {
        rfc5280::ID_KP_SERVER_AUTH => "serverAuth".to_owned(),
        rfc5280::ID_KP_CLIENT_AUTH => "clientAuth".to_owned(),
        other => oid_name(&other),
    }
}

fn public_key(spki: &x509_cert::spki::SubjectPublicKeyInfoOwned) -> String {
    let algorithm = spki.algorithm.oid;
    match algorithm {
        rfc5912::RSA_ENCRYPTION => {
            match spki
                .subject_public_key
                .as_bytes()
                .and_then(rsa_modulus_bits)
            {
                Some(bits) => format!("RSA {bits}"),
                None => "RSA".to_owned(),
            }
        }
        rfc5912::ID_EC_PUBLIC_KEY => {
            let curve = spki
                .algorithm
                .parameters
                .as_ref()
                .and_then(|params| params.decode_as::<ObjectIdentifier>().ok());
            match curve {
                Some(curve) => format!("EC {}", oid_name(&curve)),
                None => "EC".to_owned(),
            }
        }
        other => oid_name(&other),
    }
}

/// The modulus length of a PKCS#1 `RSAPublicKey`, in bits.
fn rsa_modulus_bits(key: &[u8]) -> Option<usize> {
    let mut reader = SliceReader::new(key).ok()?;
    let modulus = reader
        .sequence(|seq| {
            let modulus = UintRef::decode(seq)?;
            let _exponent = UintRef::decode(seq)?;
            Ok(modulus)
        })
        .ok()?;
    // `UintRef` strips the sign byte, so the first byte is the most
    // significant non-zero one.
    let bytes = modulus.as_bytes();
    let leading = bytes.first().map_or(0, |b| b.leading_zeros());
    bytes
        .len()
        .checked_mul(8)?
        .checked_sub(usize::try_from(leading).ok()?)
}

fn oid_name(oid: &ObjectIdentifier) -> String {
    DB.by_oid(oid)
        .map_or_else(|| oid.to_string(), str::to_owned)
}

fn ip_address(bytes: &[u8]) -> String {
    if let Ok(v4) = <[u8; 4]>::try_from(bytes) {
        return std::net::Ipv4Addr::from(v4).to_string();
    }
    if let Ok(v6) = <[u8; 16]>::try_from(bytes) {
        return std::net::Ipv6Addr::from(v6).to_string();
    }
    colon_hex(bytes)
}

fn colon_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn millis(since_epoch: std::time::Duration) -> i64 {
    i64::try_from(since_epoch.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256` with two
    /// DNS SANs, one IP SAN and `serverAuth` — the shape of a broker leaf.
    const EC_LEAF: &[u8] = include_bytes!("tls/testdata/ec-leaf.der");
    /// An RSA 2048 CA certificate, the shape of Strimzi's cluster CA.
    const RSA_CA: &[u8] = include_bytes!("tls/testdata/rsa-ca.der");

    #[test]
    fn a_broker_leaf_reads_its_names_usages_and_key() {
        let cert = CertificateDto::from_der(EC_LEAF).unwrap();
        assert_eq!(cert.subject, "CN=broker-0.kafka.example");
        assert_eq!(
            cert.dns_names,
            vec!["broker-0.kafka.example", "bootstrap.kafka.example"]
        );
        assert_eq!(cert.ip_addresses, vec!["10.0.0.7"]);
        assert_eq!(cert.extended_key_usage, vec!["serverAuth"]);
        assert!(cert.key_usage.contains(&"digitalSignature".to_owned()));
        assert_eq!(cert.is_ca, Some(false));
        assert_eq!(cert.public_key, "EC secp256r1");
        assert_eq!(cert.signature_algorithm, "ecdsa-with-SHA256");
        assert!(cert.self_signed);
        assert!(cert.not_before < cert.not_after);
    }

    #[test]
    fn an_rsa_ca_reports_its_modulus_size() {
        let cert = CertificateDto::from_der(RSA_CA).unwrap();
        assert_eq!(cert.public_key, "RSA 2048");
        assert_eq!(cert.signature_algorithm, "sha256WithRSAEncryption");
        assert_eq!(cert.is_ca, Some(true));
        assert!(cert.key_usage.contains(&"keyCertSign".to_owned()));
        assert!(cert.dns_names.is_empty());
    }

    /// Compared against `openssl x509 -fingerprint -sha256` when the fixture
    /// was generated, so the UI and the command line agree by eye.
    #[test]
    fn the_fingerprint_matches_openssl() {
        let cert = CertificateDto::from_der(EC_LEAF).unwrap();
        assert_eq!(
            cert.sha256_fingerprint,
            include_str!("tls/testdata/ec-leaf.sha256").trim()
        );
    }

    #[test]
    fn garbage_is_an_error_not_an_empty_certificate() {
        assert!(CertificateDto::from_der(b"not a certificate").is_err());
    }
}
