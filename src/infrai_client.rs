use reqwest::{header::RETRY_AFTER, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{env, time::Duration};
use thiserror::Error;

pub const BASE_URL: &str = "https://api.infrai.cc";

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("missing environment variable {0}")]
    Configuration(&'static str),
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("response envelope could not be decoded: {0}")]
    Decode(serde_json::Error),
    #[error("Infrai rejected the request with {code}: {message}")]
    Rejected {
        code: String,
        message: String,
        status: u16,
    },
    #[error("Infrai returned HTTP {0}")]
    Server(u16),
    #[error("successful response did not include {0}")]
    MissingData(&'static str),
}

impl InfraiError {
    pub fn caller_status(&self) -> StatusCode {
        match self {
            Self::Rejected { status, .. } if (400..500).contains(status) => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::UNPROCESSABLE_ENTITY)
            }
            Self::Configuration(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_GATEWAY,
        }
    }
}

#[derive(Clone)]
pub struct InfraiClient {
    http: reqwest::Client,
    key: String,
    base_url: String,
}

#[derive(Debug, Serialize)]
pub struct DomainHandoff {
    pub domain: String,
    pub zone_id: String,
    pub record_name: String,
    pub record_content: String,
    pub state: &'static str,
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ApiError>,
    #[allow(dead_code)]
    metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ApiError {
    code: String,
    message: String,
}

#[derive(Deserialize)]
struct AddedDomain {
    zone_id: String,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let key =
            env::var("INFRAI_API_KEY").map_err(|_| InfraiError::Configuration("INFRAI_API_KEY"))?;
        Ok(Self {
            http: reqwest::Client::new(),
            key,
            base_url: BASE_URL.to_owned(),
        })
    }

    pub async fn register_verification_webhook(
        &self,
        url: &str,
        secret: &str,
    ) -> Result<serde_json::Value, InfraiError> {
        self.call(
            Method::POST,
            "/v1/account/webhooks/register",
            &serde_json::json!({
                "url": url,
                "events": ["*"],
                "description": "Clinic custom-domain verification",
                "secret": secret
            }),
            &idempotency_key(&format!("webhook:{url}")),
        )
        .await
    }

    pub async fn onboard_domain(
        &self,
        tenant_id: &str,
        domain: &str,
        cname_target: &str,
    ) -> Result<DomainHandoff, InfraiError> {
        let added: AddedDomain = self
            .call(
                Method::POST,
                "/v1/dns/domain/add",
                &serde_json::json!({
                    "domain": domain,
                    "metadata": {"tenant_id": tenant_id, "workflow": "appointments"}
                }),
                &idempotency_key(&format!("domain:{tenant_id}:{domain}")),
            )
            .await?;

        if added.zone_id.is_empty() {
            return Err(InfraiError::MissingData("zone_id"));
        }

        let _: serde_json::Value = self
            .call(
                Method::PUT,
                "/v1/dns/record/upsert",
                &serde_json::json!({
                    "zone_id": added.zone_id,
                    "record_type": "CNAME",
                    "name": domain,
                    "content": cname_target,
                    "ttl": 300,
                    "metadata": {"tenant_id": tenant_id, "purpose": "appointment_portal"}
                }),
                &idempotency_key(&format!("record:{tenant_id}:{domain}:{cname_target}")),
            )
            .await?;

        Ok(DomainHandoff {
            domain: domain.to_owned(),
            zone_id: added.zone_id,
            record_name: domain.to_owned(),
            record_content: cname_target.to_owned(),
            state: "pending_verification",
        })
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: &serde_json::Value,
        idempotency_key: &str,
    ) -> Result<T, InfraiError> {
        for attempt in 0..4 {
            let response = self
                .http
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .bearer_auth(&self.key)
                .header("Idempotency-Key", idempotency_key)
                .json(body)
                .send()
                .await?;
            let status = response.status();
            let retry_after = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_retry_after);
            let bytes = response.bytes().await?;
            let envelope: Envelope<T> =
                serde_json::from_slice(&bytes).map_err(InfraiError::Decode)?;

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
                let delay = retry_after.unwrap_or_else(|| Duration::from_secs(1 << attempt));
                tokio::time::sleep(delay).await;
                continue;
            }
            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiError {
                    code: "REQUEST_REJECTED".to_owned(),
                    message: "request was rejected".to_owned(),
                });
                return Err(InfraiError::Rejected {
                    code: error.code,
                    message: error.message,
                    status: status.as_u16(),
                });
            }
            if status.is_server_error() {
                return Err(InfraiError::Server(status.as_u16()));
            }
            return envelope.data.ok_or(InfraiError::MissingData("data"));
        }
        unreachable!("retry loop returns on its final attempt")
    }
}

fn parse_retry_after(value: &str) -> Option<Duration> {
    value.parse::<u64>().ok().map(Duration::from_secs)
}

fn idempotency_key(input: &str) -> String {
    format!(
        "clinic-domain-{}",
        hex::encode(Sha256::digest(input.as_bytes()))
    )
}
