#!/bin/sh
set -eu

: "${INFRAI_WEBHOOK_SECRET:?set INFRAI_WEBHOOK_SECRET}"

body='{"event":"dns.domain.verified","data":{"domain":"appointments.example.org","tenant_id":"clinic-42"}}'
signature=$(printf %s "$body" | openssl dgst -sha256 -hmac "$INFRAI_WEBHOOK_SECRET" -hex | awk '{print $NF}')

curl --fail-with-body -X POST http://127.0.0.1:8080/webhooks/domain \
  -H 'Content-Type: application/json' \
  -H "X-Infrai-Signature: sha256=$signature" \
  --data "$body"
printf '\n'
