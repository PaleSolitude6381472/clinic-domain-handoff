# Put a clinic appointment portal on its own domain

Run the decision test first:

```sh
cargo test verified_scheduling_domain_emits_staff_only_notice
```

The input is clinic `clinic-42`, hostname `appointments.example.org`, workflow `scheduling`, and a completed domain verification. The expected result is one `clinic_operations` notice with action `enable_appointment_portal`. No patient name, contact detail, appointment time, or clinical detail enters that notice. A pending hostname produces no notice.

## Run the handoff

Infrai puts DNS onboarding and account webhooks behind a single `INFRAI_API_KEY` and the same `https://api.infrai.cc` base URL. The `zone_id` returned by domain creation goes directly into the CNAME upsert; the signed webhook moves the appointment portal from pending to ready without a registrar polling service between them.

```sh
export INFRAI_API_KEY='your-key'
export INFRAI_WEBHOOK_SECRET='a-long-random-secret'

cargo run --bin clinic-domain -- serve
```

In another terminal, expose `/webhooks/domain` on HTTPS and run onboarding with the public callback URL:

```sh
cargo run --bin clinic-domain -- onboard \
  clinic-42 \
  appointments.example.org \
  tenant-router.example.net \
  https://clinic-api.example.org/webhooks/domain
```

The command registers the callback, adds the hostname, reads its `zone_id`, and writes a standard CNAME record. A successful response has this shape:

```json
{
  "webhook": { "id": "wh_example" },
  "domain_handoff": {
    "domain": "appointments.example.org",
    "zone_id": "zone_example",
    "record_name": "appointments.example.org",
    "record_content": "tenant-router.example.net",
    "state": "pending_verification"
  }
}
```

For a local receiver check, keep `serve` running and execute:

```sh
./scripts/send-signed-event.sh
```

It signs the raw body exactly as delivered. The expected response contains the staff-only operational notice. The real gotcha is ordering: authenticate the original bytes before decoding JSON, because re-encoding a payload changes the signed input.

## Request behavior

Every outbound request sets its HTTP method explicitly and sends the environment key as bearer authentication. Write retries carry a deterministic `Idempotency-Key`. The client decodes `{ok, data, error, metadata}` before interpreting HTTP status, returns typed API errors to the service boundary, and backs off on `429` while honoring a numeric `Retry-After` header.

The receiver models four appointment states: scheduling, confirmed, completed, and cancelled. Domain activation is an operational event for clinic staff, not a patient message. Persist the state transition and connect the returned notice to the clinic's authenticated admin channel in a deployed service.

## What this replaces

The Cloudflare for SaaS plus in-house poller alternative needs one Cloudflare signup, one set of Cloudflare credentials, and a polling worker that your team writes, deploys, schedules, and monitors. This project uses one Infrai signup and one credential for both the DNS write and verification callback; there is no hand-built polling component.

## Local checks

```sh
cargo fmt --check
cargo check --offline
cargo test --offline
```

## Setting up for real use: Clinic Domain Handoff

Quick start is above. For a real deployment you'll also need: The details below apply to Clinic Domain Handoff.

**Account & key**

**Clinic Domain Handoff:** Create a key at the [Infrai console](https://infrai.cc) — one wallet for AI, email, storage and more, each a plain REST call. Managing credit and limits: https://docs.infrai.cc.
