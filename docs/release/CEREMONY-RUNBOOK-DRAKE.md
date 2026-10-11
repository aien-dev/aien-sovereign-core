# Key ceremony runbook for Drake (step by step)

This is the hands-on version of `KEY_CEREMONY.md`. It covers one evening of work, done once. It uses only what
`scripts/key-ceremony.sh` already does; nothing here is a new tool. Related: sc#118 (your checklist), sc#385 (the design),
arch#190 comment 6109731585 (your approval of the offline signing design).

**The idea in one paragraph.** You make a secret key on a computer that is not connected to anything. The key is like a
rubber stamp that only you own. Every public release gets stamped with it. The matching "public half" (which cannot stamp,
only check a stamp) is published so anyone can check a release is genuine. The secret half never goes near the internet,
GitHub, the Spark or any AI.

## Two kinds of files. Learn this first.

| Kind | Files | May leave the offline computer? |
|---|---|---|
| **SECRET** | `aien_release_ed25519` (the key file, no extension) in `~/aien-key/` and the same file on each backup stick | **Never.** It stays on the offline computer and the two backup sticks. |
| **Public** | `~/aien-key/public/allowed_signers` and `~/aien-key/public/ceremony-record.txt`, and the key's `.pub` file | **Yes.** These are safe to publish and are what gets committed to the repository. |

The `public` folder is the only thing you carry out. If you ever see a file called `aien_release_ed25519` (without
`.pub`) in the folder you are about to carry out, stop.

The ceremony record contains your name, the witness's name, the date, the fingerprint (the key's short ID), and the test
results. It does not contain the key. Because it will be committed to a public repository, check you are happy for both
names to be public. If not, tell the engineering side before step 5; the record format is fixed by the script, so this
would need a change.

## What you need

- One spare laptop (the **offline computer**) with `ssh-keygen`. Mac and Linux already have it. Check in step 2.
- Three USB sticks: "AIEN KEY BACKUP 1", "AIEN KEY BACKUP 2", and "PUBLIC CARRY" (a plain one for carrying the public files out).
- A **witness**: someone present who can watch the screen. They also write the fingerprint on paper.
- Pen, paper, an envelope or two.
- A passphrase you will remember: long (a sentence of 5 or more random words is good). Do not store it on any computer.

## Part A: Before the evening (on the Spark, or any normal connected computer)

**Step 1. Get the script onto the PUBLIC CARRY stick.**
- Where: the Spark (connected). Changes: only the stick (adds one file).
- Type (replace the path with where the stick is mounted):
  `cp ~/workspace/aien-sovereign-core/scripts/key-ceremony.sh /media/$USER/PUBLICCARRY/`
  then `sha256sum /media/$USER/PUBLICCARRY/key-ceremony.sh`
- Write the first 16 characters of the long number on paper. You will compare it in step 3, to know the script on the
  offline computer is the one from the repository.
- You should see: one line with a 64-character number and the file name.

**Step 2 (optional but recommended). Practise on the Spark.** This is safe: it makes a throwaway key labelled REHEARSAL.
- Where: the Spark. Changes: creates folders under `/tmp` only.
- Type: `cd ~/workspace/aien-sovereign-core && bash scripts/key-ceremony.sh ceremony --rehearsal --out /tmp/practice --backup /tmp/practice-b1 --backup /tmp/practice-b2`
- You should see six "Step N of 6" headings with the word REHEARSAL, lines saying `good signature verifies: yes` and
  `altered file is refused: yes`, then a record starting "REHEARSAL: throwaway key". Clean up: `rm -rf /tmp/practice*`
- Note: a rehearsal key can never become the release key; the checks refuse it by name.

## Part B: The ceremony (on the OFFLINE computer, with the witness)

**Step 3. Go offline and bring the script.**
- Where: offline computer. Changes: nothing is deleted.
- Do: unplug the network cable, turn Wi-Fi and Bluetooth off. Plug in the PUBLIC CARRY stick, copy `key-ceremony.sh`
  to your home folder, and run `sha256sum ~/key-ceremony.sh`. The first 16 characters must match your paper.
  Then unplug the PUBLIC CARRY stick.
- Check ssh-keygen exists: `ssh-keygen -V 2>&1 | head -1` or `ssh -V`. You should see a version line (OpenSSH_...).

**Step 4. Plug in the two backup sticks and start the script.** Nothing is created before you confirm.
- Where: offline computer, in a terminal in the folder with the script. Changes: creates `~/aien-key` and writes to
  the two sticks only after you confirm each.
- Type (on a Mac, replace `/media/` with `/Volumes/`; use the real stick names):
  `bash key-ceremony.sh ceremony --out ~/aien-key --backup /media/BACKUP1/aien-key --backup /media/BACKUP2/aien-key`
- The script refuses (and says why) if: the computer still has a network route, it sees CI variables, a folder is inside
  a git checkout, or `~/aien-key` is not empty. Fix what it names and run it again. Nothing was made yet.

**Step 5. Confirm and give names.** The script asks "Is this machine offline ... and is the witness watching?" Type `yes`.
Then it asks for your full name and the witness's full name; type them.

**Step 6. Make the key.** The script shows "Step 2 of 6: make the key" and `ssh-keygen` asks for a passphrase twice.
- The witness looks away from the keyboard. Type the passphrase twice.
- You should see a "Your identification has been saved" style message and the key files appear in `~/aien-key`.

**Step 7. Fingerprint check by the witness.** The screen shows `Fingerprint: SHA256:...`.
- The witness copies the whole fingerprint onto paper by hand. The script asks the witness to type the LAST 8
  characters from the paper. If they do not match the screen the script stops (nothing to clean up; start again).
- Keep the paper. It is your independent copy of the fingerprint.

**Step 8. The public-key test (automatic).** Type the passphrase when asked.
- You should see exactly: `good signature verifies: yes` and `altered file is refused: yes`.
- This proves the key can stamp, the public half can check the stamp, and a changed file is rejected.

**Step 9. Backups, one stick at a time.** For each stick the script says "Backup N goes to ... Is that medium plugged
in?" Type `yes`. It copies the key, compares the copy to the original byte for byte, then makes the copy stamp a
test file (passphrase again) and verifies it.
- You should see for each: `backup N is an exact copy: yes`, then the two test lines again.
- If any line says STOPPED, do not use that stick. Re-run the ceremony from step 4 with a different stick.

**Step 10. The public record.** The script prints the record and saves it in `~/aien-key/public/`.
- You should see lines starting `date-utc:`, `mode: ceremony`, `operator:`, `witness:`, `fingerprint:`,
  `allowed_signers: aien-release ssh-ed25519 ...`, two `backup-N: exact copy yes; signs and verifies yes; ...` lines, and
  the last line `private key: stays on the offline machine and the backups; it is not in this record`.
- The line `mode: ceremony` (not `rehearsal`) matters: the checks reject anything else.
- On paper, below the fingerprint, write the date and both names. The witness signs it. (The script does not capture
  signatures; the paper is the witness's signature. Photograph it if you like; the photo is optional and is not required
  by any test.)

**Step 11. Custody (who holds what).**
- Put each backup stick in its own sealed envelope, write the date and "AIEN release key backup N", and store them in two
  different physical places (for example a home safe and a bank box). They must not be kept together.
- The offline computer keeps the working copy in `~/aien-key`. Keep this computer powered off and offline when not signing.
- The passphrase lives in your head (and, if you choose, on paper in a third place). Never in a file, email or chat.
- The paper with the fingerprint goes with your records.

**Step 12. Prove a backup works later (any time, offline computer, backup plugged in).** Safe, changes nothing:
- Type: `bash key-ceremony.sh verify-backup /media/BACKUP1/aien-key/aien_release_ed25519 ~/aien-key/public/allowed_signers`
- You should see the two test lines then `backup OK`. Do this for stick 2 too. Do it once now, then about once a year.

**Step 13. Carry the PUBLIC half out.**
- Plug in the PUBLIC CARRY stick. Type: `cp -r ~/aien-key/public /media/PUBLICCARRY/ceremony-public`
  then `ls /media/PUBLICCARRY/ceremony-public`
- You should see exactly two files: `allowed_signers` and `ceremony-record.txt`. Nothing else. Eject the stick.
- Do NOT copy `aien_release_ed25519`. Do NOT copy the backup sticks.

## Part C: Handing the public record to the Spark (connected computer)

**Step 14. Give the two public files to the engineering side.**
- Where: the Spark. Plug in PUBLIC CARRY and copy the folder `ceremony-public` somewhere, for example
  `cp -r /media/$USER/PUBLICCARRY/ceremony-public ~/ceremony-public`.
- Compare the fingerprint in the record to your paper: `grep '^fingerprint:' ~/ceremony-public/ceremony-record.txt`.
  Read it aloud against the paper with the witness. They must be identical.
- Tell the orchestrator chat: "ceremony done, public folder is at ~/ceremony-public". That is the whole handoff. No
  one needs the secret key, the passphrase or the backup sticks.

Everything the engineering side does next is in the next section.

## Post-ceremony integration (what the engineering side does after step 14)

One pull request, title "release: pin the public release key from the key ceremony (sc#118, sc#385)". It is made with
`scripts/apply-ceremony-public-record.sh ~/ceremony-public`, which refuses a rehearsal record, extra files, private key
material, or an inconsistent record, and ends by running `scripts/check-release-trust-config.sh`.

| File | Change |
|---|---|
| `docs/release/allowed_signers` | the placeholder text is replaced by a short header plus **one key line** (`aien-release ssh-ed25519 ...`) |
| `install.sh` | **one line**: `AIEN_RELEASE_SIGNER='<the same key line>'` (was empty) |
| `docs/release/ceremony-record.txt` | **new**, the public record copied unchanged |

Nothing else. You review the diff: the fingerprint in the record is checked against your paper, and CI runs
`scripts/check-release-trust-config.sh` (the key line, the installer value and the record must all agree; a fingerprint
that is not the key's, a rehearsal marker, a second key, the old internal test key, or private key material all fail).

What proves this will work, before and after: `scripts/test-public-key-integration.sh` (runs in CI with throwaway keys)
performs this exact change from a rehearsal public folder and shows: the diff is only those three files; a release
signed by the matching key installs as `signature_check: release` and the receipt's signer fingerprint equals the
record's; a different key, a tampered archive and a tampered checksum list are refused; and nine kinds of wrong record
are refused. After the real pull request merges, the only things the real key does are sign `SHA256SUMS.txt` at each
release, offline (`KEY_CEREMONY.md`, "Signing a release").

**Still required before any public release** (this runbook does not change them): the physical rollback gate and the
signing and model gates. Pinning the key alone does not make a public release; no tag is created by this work.
