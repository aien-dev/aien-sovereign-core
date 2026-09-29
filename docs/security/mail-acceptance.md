# Mail access and transport outcomes

## Access

The mail HTTP API (`127.0.0.1:18092`) requires `Authorization: Bearer <token>`. Provision `AIEN_MAIL_API_TOKEN` with `atlas-vault add AIEN_MAIL_API_TOKEN` using standard input. The daemon resolves the token from process memory or the TPM-bound vault (PATH first, then `~/.local/bin/atlas-vault`, since the service PATH omits it) and caches a vault hit for 60 seconds. Plaintext token files are not supported. Missing credentials fail closed. `/health` stays public for monitors. Browser requests (any `Origin` header) are refused; the cockpit forwards the separately provisioned mail token.


## Transport outcome

The local bridge is submitted through lettre with a five-second socket timeout. A message is saved to the "sent" folder and offered to Cortex only after the bridge's final `250` reply to end-of-DATA. Unavailable bridges, refused recipients, interrupted connections and negative DATA replies return an error before anything is saved as sent or indexed. Successful responses carry `X-AIEN-Transport-Status: smtp_accepted` in message headers. This means SMTP acceptance, not recipient delivery. Connection loss during DATA can leave acceptance unknown; do not automatically retry an unknown outcome.

Sender, recipient and subject values containing CR, LF or NUL are refused before any policy check or network use, so a caller cannot inject SMTP commands or extra headers.

If SMTP accepts but local persistence fails, the response remains accepted and carries `X-AIEN-Storage-Status: persist_failed`. Reporting a send failure at that point would invite duplicate mail. Cortex indexing remains separately reported through cortex_indexed.

Tests point the Cortex bridge at a port with nothing listening, never at the live Cortex on `127.0.0.1:18080`.
