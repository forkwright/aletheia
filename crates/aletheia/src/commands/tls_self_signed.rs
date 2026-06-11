//! Self-signed X.509 certificate generation.
//!
//! Hand-rolled replacement for the previous third-party cert generator.
//! Uses `p256` for ECDSA P-256 key generation + signing and hand-rolled ASN.1 DER.

#![expect(
    clippy::indexing_slicing,
    reason = "ASN.1 DER encoding requires precise byte-level manipulation with manually verified bounds"
)]
#![expect(
    clippy::as_conversions,
    reason = "encoding small fixed-size integers to u8 with known-good bounds"
)]
#![expect(
    clippy::cast_possible_truncation,
    reason = "encoding small fixed-size integers to u8 with known-good bounds"
)]

use std::net::IpAddr;
use std::str::FromStr;

use jiff::Timestamp;
use p256::ecdsa::signature::Signer as _;
use p256::ecdsa::{DerSignature, SigningKey};
use p256::elliptic_curve::pkcs8::EncodePrivateKey;
use rand_core::{OsRng, RngCore as _};
use snafu::prelude::*;

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to generate key pair"))]
    KeyGen,
    #[snafu(display("failed to parse key pair"))]
    KeyParse,
    #[snafu(display("failed to sign certificate"))]
    Sign,
    #[snafu(display(
        "tls generate: --days {days} is too large; certificate expiry would overflow the representable timestamp range"
    ))]
    ExpiryOverflow { days: u32 },
}

/// A freshly generated self-signed certificate and its private key, both PEM-encoded.
pub(crate) struct SelfSignedCert {
    pub(crate) cert_pem: String,
    pub(crate) key_pem: String,
}

/// Generate a self-signed X.509 v3 certificate valid for `days` from now.
pub(crate) fn generate(
    sans: &[String],
    days: u32,
    common_name: &str,
) -> Result<SelfSignedCert, Error> {
    let mut rng = OsRng;

    // ---- key pair ----
    let signing_key = SigningKey::random(&mut rng);
    let pkcs8_doc = signing_key.to_pkcs8_der().map_err(|_e| Error::KeyGen)?;

    let pub_point = signing_key.verifying_key().to_encoded_point(false);
    let pub_key = pub_point.as_bytes();

    // ---- times ----
    let now = Timestamp::now();
    let end = now
        .checked_add(jiff::SignedDuration::from_hours(i64::from(days) * 24))
        .map_err(|_e| Error::ExpiryOverflow { days })?;

    // ---- serial number (16 random bytes, positive) ----
    let mut serial = [0u8; 16];
    rng.fill_bytes(&mut serial);
    serial[0] &= 0x7f; // ensure positive

    // ---- build TBS certificate ----
    let mut tbs = Writer::new();
    tbs.sequence(|writer| {
        // version [0] INTEGER 2  (v3)
        writer.context_tagged(0, |w| w.integer(2));
        // serialNumber
        writer.big_int_positive(&serial);
        // signature algorithm
        writer.sequence(|w| {
            w.oid(&[1, 2, 840, 10045, 4, 3, 2]); // ecdsa-with-SHA256
        });
        // issuer
        write_name(writer, common_name);
        // validity
        writer.sequence(|w| {
            write_time(w, now);
            write_time(w, end);
        });
        // subject
        write_name(writer, common_name);
        // subjectPublicKeyInfo
        writer.sequence(|w| {
            w.sequence(|w2| {
                w2.oid(&[1, 2, 840, 10045, 2, 1]); // ecPublicKey
                w2.oid(&[1, 2, 840, 10045, 3, 1, 7]); // prime256v1
            });
            w.bit_string(pub_key, 0);
        });
        // extensions [3]
        writer.context_tagged(3, |w| {
            w.sequence(|w2| {
                // SubjectAltName
                w2.sequence(|w3| {
                    w3.oid(&[2, 5, 29, 17]);
                    w3.octet_string_in_sequence(|w4| {
                        w4.sequence(|w5| {
                            for san in sans {
                                write_general_name(w5, san);
                            }
                        });
                    });
                });
                // BasicConstraints CA=true
                w2.sequence(|w3| {
                    w3.oid(&[2, 5, 29, 19]);
                    w3.octet_string_in_sequence(|w4| {
                        w4.sequence(|w5| w5.bool(true));
                    });
                });
                // KeyUsage: digitalSignature, keyCertSign, cRLSign
                w2.sequence(|w3| {
                    w3.oid(&[2, 5, 29, 15]);
                    w3.bool(true);
                    w3.octet_string_in_sequence(|w4| {
                        // bits 0,5,6 set => 0b1000_0110 = 0x86; 7 bits used
                        w4.bit_string(&[0x86], 1);
                    });
                });
            });
        });
    });

    let tbs_der = tbs.finish();

    // ---- sign ----
    let sig: DerSignature = signing_key.try_sign(&tbs_der).map_err(|_e| Error::Sign)?;

    // ---- assemble certificate ----
    let mut cert = Writer::new();
    cert.sequence(|writer| {
        writer.raw(&tbs_der);
        writer.sequence(|w| {
            w.oid(&[1, 2, 840, 10045, 4, 3, 2]); // ecdsa-with-SHA256
        });
        writer.bit_string(sig.as_bytes(), 0);
    });

    let cert_pem = pem::encode(&pem::Pem::new("CERTIFICATE", cert.finish()));
    let key_pem = pem::encode(&pem::Pem::new("PRIVATE KEY", pkcs8_doc.as_bytes().to_vec()));

    Ok(SelfSignedCert { cert_pem, key_pem })
}

// ── ASN.1 helpers ──
struct Writer(Vec<u8>);

impl Writer {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn finish(self) -> Vec<u8> {
        self.0
    }
    fn raw(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }
    fn write(&mut self, tag: u8, contents: &[u8]) {
        self.0.push(tag);
        write_length(&mut self.0, contents.len());
        self.0.extend_from_slice(contents);
    }
    fn sequence(&mut self, f: impl FnOnce(&mut Self)) {
        let mut inner = Self::new();
        f(&mut inner);
        let bytes = inner.finish();
        self.write(0x30, &bytes);
    }
    fn context_tagged(&mut self, tag: u8, f: impl FnOnce(&mut Self)) {
        let mut inner = Self::new();
        f(&mut inner);
        let bytes = inner.finish();
        self.write(0xa0 | tag, &bytes);
    }
    fn set(&mut self, f: impl FnOnce(&mut Self)) {
        let mut inner = Self::new();
        f(&mut inner);
        let bytes = inner.finish();
        self.write(0x31, &bytes);
    }
    fn integer(&mut self, n: u64) {
        let mut buf = [0u8; 8];
        let mut len = 0;
        let mut n = n;
        loop {
            buf[7 - len] = (n & 0xff) as u8;
            len += 1;
            n >>= 8;
            if n == 0 {
                break;
            }
        }
        let bytes = &buf[8 - len..];
        if bytes[0] & 0x80 != 0 {
            let mut v = vec![0];
            v.extend_from_slice(bytes);
            self.write(0x02, &v);
        } else {
            self.write(0x02, bytes);
        }
    }
    fn big_int_positive(&mut self, bytes: &[u8]) {
        let mut start = 0;
        while start < bytes.len() - 1 && bytes[start] == 0 {
            start += 1;
        }
        let bytes = &bytes[start..];
        if bytes[0] & 0x80 != 0 {
            let mut v = vec![0];
            v.extend_from_slice(bytes);
            self.write(0x02, &v);
        } else {
            self.write(0x02, bytes);
        }
    }
    fn oid(&mut self, oid: &[u64]) {
        let mut bytes = Vec::new();
        bytes.push((oid[0] * 40 + oid[1]) as u8);
        for &n in &oid[2..] {
            let mut n = n;
            let mut parts = Vec::new();
            loop {
                parts.push((n & 0x7f) as u8);
                n >>= 7;
                if n == 0 {
                    break;
                }
            }
            for i in (1..parts.len()).rev() {
                bytes.push(parts[i] | 0x80);
            }
            bytes.push(parts[0]);
        }
        self.write(0x06, &bytes);
    }
    fn bool(&mut self, b: bool) {
        // DER requires TRUE to be encoded as 0xFF, not 0x01.
        self.write(0x01, &[if b { 0xff } else { 0x00 }]);
    }
    fn bit_string(&mut self, data: &[u8], unused_bits: u8) {
        let mut v = vec![unused_bits];
        v.extend_from_slice(data);
        self.write(0x03, &v);
    }
    fn octet_string_in_sequence(&mut self, f: impl FnOnce(&mut Self)) {
        let mut inner = Self::new();
        f(&mut inner);
        let bytes = inner.finish();
        self.write(0x04, &bytes);
    }
}

fn write_length(out: &mut Vec<u8>, len: usize) {
    if len < 128 {
        out.push(len as u8);
    } else {
        let mut n = len;
        let mut buf = [0u8; 8];
        let mut idx = 8;
        while n > 0 {
            idx -= 1;
            buf[idx] = (n & 0xff) as u8;
            n >>= 8;
        }
        out.push(0x80 | (8 - idx) as u8);
        out.extend_from_slice(&buf[idx..]);
    }
}

fn write_name(writer: &mut Writer, cn: &str) {
    writer.sequence(|w| {
        w.set(|w2| {
            w2.sequence(|w3| {
                w3.oid(&[2, 5, 4, 3]); // commonName
                w3.write(0x0c, cn.as_bytes()); // UTF8String
            });
        });
    });
}

fn write_time(writer: &mut Writer, ts: Timestamp) {
    // RFC 5280: UTCTime for dates before 2050, GeneralizedTime otherwise.
    let dt = ts.to_zoned(jiff::tz::TimeZone::UTC).datetime();
    let (year, month, day, hour, minute, second) = (
        dt.year(),
        dt.month(),
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
    );
    if year < 2050 {
        let s = format!(
            "{:02}{:02}{:02}{:02}{:02}{:02}Z",
            year % 100,
            month,
            day,
            hour,
            minute,
            second
        );
        writer.write(0x17, s.as_bytes()); // UTCTime
    } else {
        let s = format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}Z");
        writer.write(0x18, s.as_bytes()); // GeneralizedTime
    }
}

fn write_general_name(writer: &mut Writer, san: &str) {
    if let Ok(ip) = IpAddr::from_str(san) {
        let bytes = match ip {
            IpAddr::V4(addr) => addr.octets().to_vec(),
            IpAddr::V6(addr) => addr.octets().to_vec(),
        };
        writer.write(0x87, &bytes); // context [7] OCTET STRING
    } else {
        writer.write(0x82, san.as_bytes()); // context [2] IA5String (dNSName)
    }
}
