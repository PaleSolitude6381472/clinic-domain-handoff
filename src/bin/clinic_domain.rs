use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use clinic_domain_handoff::{
    decide_notification, verify_signature, AppointmentWorkflow, DomainVerifiedEvent, InfraiClient,
    InfraiError,
};
use serde_json::json;
use std::{env, sync::Arc};

#[derive(Clone)]
struct WebhookState {
    secret: Arc<[u8]>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), ServiceError> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("onboard") if args.len() == 6 => {
            let client = InfraiClient::from_env()?;
            let secret = required_env("INFRAI_WEBHOOK_SECRET")?;
            let handoff = client.onboard_domain(&args[2], &args[3], &args[4]).await?;
            let webhook = client
                .register_verification_webhook(&args[5], &secret)
                .await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "webhook": webhook,
                    "domain_handoff": handoff
                }))?
            );
            Ok(())
        }
        Some("serve") => serve().await,
        _ => Err(ServiceError::Usage),
    }
}

async fn serve() -> Result<(), ServiceError> {
    let secret = required_env("INFRAI_WEBHOOK_SECRET")?;
    let state = WebhookState {
        secret: Arc::from(secret.into_bytes()),
    };
    let app = Router::new()
        .route("/webhooks/domain", post(domain_verified))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
    println!("listening on http://127.0.0.1:8080");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn domain_verified(
    State(state): State<WebhookState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ServiceError> {
    let signature = headers
        .get("x-infrai-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or(ServiceError::MissingSignature)?;
    verify_signature(&body, signature, &state.secret)?;

    let event: DomainVerifiedEvent = serde_json::from_slice(&body)?;
    if event.event != "dns.domain.verified" {
        return Ok((StatusCode::ACCEPTED, Json(json!({"action": "ignored"}))));
    }
    let notice = decide_notification(
        &event.data.tenant_id,
        &event.data.domain,
        &AppointmentWorkflow::Scheduling,
        true,
    );
    Ok((StatusCode::ACCEPTED, Json(json!({"notification": notice}))))
}

fn required_env(name: &'static str) -> Result<String, ServiceError> {
    env::var(name).map_err(|_| ServiceError::MissingEnvironment(name))
}

#[derive(Debug, thiserror::Error)]
enum ServiceError {
    #[error("usage: clinic-domain serve | clinic-domain onboard TENANT_ID DOMAIN CNAME_TARGET WEBHOOK_URL")]
    Usage,
    #[error("missing environment variable {0}")]
    MissingEnvironment(&'static str),
    #[error(transparent)]
    Infrai(#[from] InfraiError),
    #[error("invalid webhook signature: {0}")]
    Signature(#[from] clinic_domain_handoff::SignatureError),
    #[error("webhook signature header is missing")]
    MissingSignature,
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Infrai(error) => error.caller_status(),
            Self::Signature(_) | Self::MissingSignature => StatusCode::UNAUTHORIZED,
            Self::Json(_) | Self::Usage => StatusCode::BAD_REQUEST,
            Self::MissingEnvironment(_) | Self::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({"error": self.to_string()}))).into_response()
    }
}
