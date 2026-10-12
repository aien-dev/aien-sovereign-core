Valid but not the daemon's path: the smoke binary at this point did not apply the daemon's marker-wait spin window
(AIEN_OMEGA_SPIN_US unset, omega default 0 = 50 us sleep-poll), so every launch paid the sleep-poll floor.
Results (03:30-03:33Z, one hold, rc 0, p1/p2 equal to the golden, fallback 0, p3 256 tokens):
budget 256: 40223 ms, 6.36 tok/s; budget 1024: 37825 ms, 6.77 tok/s; identical p3 ids.
Kept as the record of why the smoke example now applies DAEMON_DEFAULT_SPIN_US as the daemon does.
