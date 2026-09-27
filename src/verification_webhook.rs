use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Deserialize)]
pub struct DomainVerifiedEvent {
    pub event: String,
    pub data: VerifiedDomain,
}

#[derive(Debug, Deserialize)]
pub struct VerifiedDomain {
    pub domain: String,
    pub tenant_id: String,
}

#[derive(Debug, Error)]
pub enum SignatureError {
    #[error("signature is not valid hex")]
    InvalidEncoding,
    #[error("signature does not match the request body")]
    Mismatch,
    #[error("signing key could not be initialized")]
    InvalidSecret,
}

pub fn verify_signature(body: &[u8], supplied: &str, secret: &[u8]) -> Result<(), SignatureError> {
    let digest = supplied
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(supplied.trim());
    let expected = hex::decode(digest).map_err(|_| SignatureError::InvalidEncoding)?;
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| SignatureError::InvalidSecret)?;
    mac.update(body);
    mac.verify_slice(&expected)
        .map_err(|_| SignatureError::Mismatch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_covers_the_raw_payload() {
        let body = br#"{"event":"dns.domain.verified","data":{"domain":"appointments.example.org","tenant_id":"clinic-42"}}"#;
        let mut mac = HmacSha256::new_from_slice(b"local-test-secret").unwrap();
        mac.update(body);
        let signature = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));

        assert!(verify_signature(body, &signature, b"local-test-secret").is_ok());
        assert!(verify_signature(b"{}", &signature, b"local-test-secret").is_err());
    }
}
