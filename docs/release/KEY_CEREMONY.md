# Release key ceremony (plain-English guide)

This is how the AIEN public-release key gets made. It happens once. Drake (the operator) does it with one witness
watching. The script `scripts/key-ceremony.sh` walks through every step on screen and stops if anything is wrong.
Operator checklist: sc#118. Ruling: Drake ruling 1, sc#385.

**What the key is.** A digital seal. Every public release gets a small seal file (`SHA256SUMS.txt.sig`). Anyone's
computer can check that seal against the public half of the key, which is published in this repository. Only the
private half can make a seal, so the private half must never touch the internet.

**What is human-only.** Everything in this guide. No AI, CI job or server makes, holds or uses this key. The steps an
AI may do are listed at the end.

## What you need

- One computer that will stay **offline** for the ceremony: network cable out, Wi-Fi off. A spare laptop is fine.
  It needs `ssh-keygen` (part of OpenSSH; on a Mac or Linux it is already there) and a copy of this repository's
  `scripts/key-ceremony.sh` (copy it on a USB stick before you unplug the network).
- Two USB sticks for the backups (label them "AIEN KEY BACKUP 1" and "2"), and a third for carrying public files out.
- A witness, a pen and paper.
- A passphrase you can remember: long, written nowhere the key is kept.

## Practise first (optional, five minutes, safe anywhere)

```sh
bash scripts/key-ceremony.sh ceremony --rehearsal --out /tmp/practice --backup /tmp/practice-b1 --backup /tmp/practice-b2
```

It makes a throwaway key labelled REHEARSAL everywhere. It is never a release key. Delete `/tmp/practice*` after.

## The ceremony

1. **Go offline.** Unplug the network cable and turn Wi-Fi off. Plug in the two backup sticks. The witness watches the
   screen from here on.
2. **Start the script** in a terminal, in the folder holding the script:
   ```sh
   bash key-ceremony.sh ceremony --out ~/aien-key --backup /media/BACKUP1/aien-key --backup /media/BACKUP2/aien-key
   ```
   (On a Mac the sticks appear under `/Volumes/` instead of `/media/`.) If the computer still has a network route, or
   any folder is inside a code checkout, the script stops and tells you why. Nothing is made yet.
3. **Confirm and give names.** Type `yes` to confirm you are offline and the witness is watching, then type your name
   and the witness's name.
4. **Make the key.** `ssh-keygen` asks for the passphrase twice. The witness does not look at the keyboard.
5. **Fingerprint.** The screen shows a line like `SHA256:Ab3...xYz`. The witness copies it onto paper by hand, then
   types its last 8 characters back in. If they do not match, the script stops.
6. **Public-key verification test (automatic).** The key seals a fresh test file, the seal is checked against the
   public key, and then a copy of the file with one letter changed must be **refused**. You will see:
   `good signature verifies: yes` and `altered file is refused: yes`. You type the passphrase once here.
7. **Verified backups.** For each stick, type `yes` when it is plugged in. The key is copied, compared byte for byte,
   and the copy on the stick must seal and verify on its own (passphrase again). You will see
   `backup 1 is an exact copy: yes` and the two test lines, then the same for backup 2.
8. **The public record.** The script prints the record and saves it with the public key in `~/aien-key/public/`
   (`allowed_signers` and `ceremony-record.txt`). Nothing secret is in that folder. The witness signs the paper
   with the fingerprint, date and both names.
9. **Carry out the public half only.** Copy the folder `~/aien-key/public` to the third stick. Never copy the key
   file itself. Store the two backup sticks in two different safe places.

## After the ceremony

Step-by-step version for the operator: `CEREMONY-RUNBOOK-DRAKE.md`.

- Hand `public/allowed_signers` and `public/ceremony-record.txt` to the orchestrator. It opens one pull request that
  puts that key line into `docs/release/allowed_signers` and `install.sh` (`AIEN_RELEASE_SIGNER`), and the tests prove
  a release sealed with it installs as `signature_check: release`. You check the fingerprint in that pull request
  against your paper before it is merged.
- Any time later, check a backup still works (offline machine, stick plugged in):
  `bash key-ceremony.sh verify-backup /media/BACKUP1/aien-key/aien_release_ed25519 ~/aien-key/public/allowed_signers`
  You should see `backup OK`.

## Signing a release (each release, human only)

1. CI drafts the release and its `SHA256SUMS.txt` (not public yet).
2. Copy `SHA256SUMS.txt` to a stick, take it to the offline machine and run
   `bash key-ceremony.sh sign ~/aien-key/aien_release_ed25519 ~/aien-key/public/allowed_signers SHA256SUMS.txt`.
   It lists the files the seal vouches for; type `yes` if they are the release you mean, then the passphrase. You
   should see `signed and verified: SHA256SUMS.txt.sig`.
3. Carry `SHA256SUMS.txt.sig` back and attach it to the draft release on GitHub. Then the publish workflow checks the
   seal and publishes; if anything does not match, it refuses and nothing is published.

## What an AI may do, and what it never does

May: write and test this script with throwaway REHEARSAL keys, prepare the pull request that pins the public key,
run the publish workflow after you have attached the seal. Never: make, see, copy, store or use the real private key;
claim a ceremony happened; create a public release tag.
