# Cockpit operator access

The cockpit refuses to start without an access token. Provision `AIEN_COCKPIT_TOKEN` in atlas-vault, or supply it through the process environment in memory. Use a random token of at least 32 ASCII characters. Tokens are never accepted in URLs.

Default listener: `127.0.0.1:18095`. `AIEN_COCKPIT_ADDR` explicitly changes the listener. Browser origins default to `http://127.0.0.1:18095` and `http://localhost:18095`; configure exact origins through comma-separated `AIEN_COCKPIT_ORIGINS`. Use HTTPS for remote browser access. The terminal additionally retains its loopback peer and localhost Origin restriction; remote origins cannot open a shell.

Open `/login` and enter the operator token. Sign-in creates a random, HttpOnly, SameSite=Strict session with a server-enforced one-hour lifetime. HTTPS origins receive Secure cookies. Restarting the service clears sessions. API callers use `Authorization: Bearer <token>`. Every route except sign-in is protected, including artifacts, service controls and terminal upgrades. Browser mutations and terminal upgrades require an allowed Origin. The token authorizes the operator's administrative surface, not an untrusted agent principal.

The cockpit forwards mail requests using a separately provisioned `AIEN_MAIL_API_TOKEN`. Missing mail credentials cause the mail daemon to refuse API access. A legacy service installation must provision both tokens before starting the updated services. Nothing in this change deploys a service or changes a firewall.
