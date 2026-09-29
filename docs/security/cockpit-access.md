# Cockpit operator access

## Opening the cockpit on the Spark (plain steps)

The cockpit now asks for a password-like access token before it shows anything or lets anyone press a button. Only the Spark itself can reach it.

1. Open a terminal on the Spark and type:

   ```
   cat ~/.config/aien/cockpit.token
   ```

   It prints one long line of letters and numbers. Select it and copy it. That line is the cockpit key; do not paste it into chats, issues or screenshots.
2. Open a web browser on the Spark and go to `http://127.0.0.1:18095` (or `http://localhost:18095`).
3. The browser shows a page titled "Sign in to AIEN". Paste the key into the box and press "Sign in".
4. The normal cockpit appears. The sign-in lasts one hour, and restarting the cockpit service also signs you out. After that, the browser returns to the sign-in page; repeat steps 1 to 3.

From the MacBook, open a tunnel first (Terminal on the Mac: `ssh -L 18095:127.0.0.1:18095 <spark>`, leave it open), then follow steps 2 to 4 in a Mac browser at `http://127.0.0.1:18095`. The LAN address and the Tailscale address no longer work on purpose.

## Where the keys live

| Key | File | Who reads it |
| --- | --- | --- |
| Cockpit operator token | `~/.config/aien/cockpit.token` | spark-cockpit-rs at start |
| Mail API token | `~/.config/aien/mail-api.token` | spark-mail (checks it) and the cockpit (sends it when forwarding mail requests) |

Both files hold one random line of at least 32 characters, owned by the operator account, mode `0600` (owner read and write only). A service refuses a file that group or others can read. The files live outside every repository and are never committed. Tokens are never printed to logs and never accepted in URLs.

Lookup order for each token: the process environment (`AIEN_COCKPIT_TOKEN`, `AIEN_MAIL_API_TOKEN`), then the file above, then `atlas-vault get <NAME>`. With none present, the cockpit refuses to start and the mail API refuses every request.

Create or rotate a token (then restart the service that reads it):

```
umask 077; mkdir -p ~/.config/aien
head -c 48 /dev/urandom | base64 | tr -d '/+=\n' > ~/.config/aien/cockpit.token
chmod 600 ~/.config/aien/cockpit.token
```

## What is protected

- Listener: `127.0.0.1:18095` by default. `AIEN_COCKPIT_ADDR` changes it explicitly.
- Allowed browser origins: `http://127.0.0.1:18095` and `http://localhost:18095`. `AIEN_COCKPIT_ORIGINS` (comma separated, exact origins) replaces the list. Use HTTPS for any remote browser origin.
- Every route except `/login` and `/auth/session` needs either `Authorization: Bearer <token>` or a signed-in browser session. Browser sessions are random, HttpOnly, SameSite=Strict cookies with a server-enforced one-hour lifetime; HTTPS origins also get Secure cookies.
- Browser mutations (anything other than GET/HEAD) and terminal upgrades additionally need an allowed Origin. A request carrying a foreign Origin gets 403.
- The terminal (`/ws/terminal`) keeps its own check on top: exact loopback Origin and a loopback peer address. Remote origins cannot open a shell.
- Health reads stay open for local monitors: `GET /api/pulse` and `GET /api/status` answer without a token, but only when the request's Host is `127.0.0.1`, `localhost` or `[::1]` (blocks DNS-rebinding pages) and it carries no foreign Origin. aien-cli, spark-debugger, spark-supervisor and bench_inference_stack only use these reads.

The token authorizes the operator's administrative surface, not an untrusted agent principal.
