# Mail access and transport outcomes

The mail HTTP API requires `Authorization: Bearer <token>`, resolved from `AIEN_MAIL_API_TOKEN` in the process environment or atlas-vault. Use a random token of at least 32 characters. Missing credentials fail closed. Health is public. Browser requests are refused; access mail through the authenticated cockpit, which forwards the separately provisioned token.

The local bridge is submitted through lettre with a five-second socket timeout. Unavailable bridges, refused recipients, interrupted connections and negative DATA replies return an error before anything is saved as sent or indexed as accepted. Successful responses carry `X-AIEN-Transport-Status: smtp_accepted` in message headers. This means SMTP acceptance, not recipient delivery. Connection loss during DATA can leave acceptance unknown; do not automatically retry an unknown outcome.

If SMTP accepts but local persistence fails, the response remains accepted and carries `X-AIEN-Storage-Status: persist_failed`. Reporting a send failure at that point would invite duplicate mail. Cortex indexing remains separately reported through cortex_indexed.
