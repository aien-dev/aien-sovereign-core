---
name: self-improvement
description: Autonomous self-improvement protocol for AIEN: isolated git worktree sandboxing, Microsoft SkillOpt optimization, and headless browser mirror self-testing via Chrome DevTools Protocol.
---

# Autonomous Self-Improvement & Browser Mirror Protocol

Use this skill when modifying AIEN core code, improving skills via Microsoft SkillOpt, or verifying live UI and streaming behavior using the headless browser mirror harness.

## Core Pillars of Sovereign Self-Improvement

1. **Never Break Live Harnesses**:
   Live systemd services (`spark-cockpit.service`, `~/.local/bin/aien`) must never be edited directly. All exploratory code modifications, refactorings, and experiments must take place inside an isolated git worktree sandbox.
2. **Deterministic IPC over Flaky Webhooks**:
   Browser self-testing uses Chrome DevTools Protocol (CDP) WebSocket on port 9222 and local Unix Domain Sockets (UDS). Webhooks are strictly forbidden for local control loops.
3. **Dual-Seat Mirror Auditing**:
   AIEN tests his live Cockpit UI by driving headless Chrome as an adversarial auditor, verifying page hydration, live token streaming via Axum SSE, tactile action buttons, and tab transitions.

## 1. The Isolated Git Worktree Sandbox Workflow

When tasked with improving or fixing your own codebase (`aien-sovereign-core`):

### Step 1: Initialize the Worktree Sandbox
```bash
git -C /home/drakestapleton/workspace/aien-sovereign-core worktree add /home/drakestapleton/workspace/aien-sandbox -b sandbox-eval
cd /home/drakestapleton/workspace/aien-sandbox
```

### Step 2: Make Edits & Run Unit Tests
Apply modifications inside `~/workspace/aien-sandbox`. Verify that all compiler checks and tests pass:
```bash
source ~/.cargo/env
cargo test --workspace
cargo build --release
```

### Step 3: Run the Browser Mirror Self-Test
Ensure UI and streaming endpoints remain 100% operational:
```bash
~/max-env/bin/python /home/drakestapleton/basecamp/scripts/browser_mirror_test.py
```
Check `/home/drakestapleton/basecamp/ui-tests/report.json` to confirm `"status": "PASSED"`.

### Step 4: Promote and Clean Up
If all tests and mirror assertions pass, merge the branch to `main`, update the installed release binary, and remove the worktree:
```bash
git -C /home/drakestapleton/workspace/aien-sovereign-core merge sandbox-eval
cp /home/drakestapleton/workspace/aien-sovereign-core/crates/aien-cli/target/release/aien-cli ~/.local/bin/aien
git -C /home/drakestapleton/workspace/aien-sovereign-core worktree remove /home/drakestapleton/workspace/aien-sandbox
git -C /home/drakestapleton/workspace/aien-sovereign-core branch -d sandbox-eval
```

If tests fail, abort cleanly:
```bash
git -C /home/drakestapleton/workspace/aien-sovereign-core worktree remove --force /home/drakestapleton/workspace/aien-sandbox
git -C /home/drakestapleton/workspace/aien-sovereign-core branch -D sandbox-eval
```

## 2. Microsoft SkillOpt Self-Evolution Loop

To optimize a sovereign skill against a held-out benchmark split using the local Modular MAX model seat (port 18006):

```bash
bash /home/drakestapleton/atlas-skillopt-stage.sh
```

### Workflow
1. Configures `SkillOpt` with optimizer and target model set to `nvidia/NVIDIA-Nemotron-3.5-Lightning-30B-A3B-BF16` on `http://127.0.0.1:18006/v1`.
2. Materializes benchmark splits without external telemetry (`DO_NOT_TRACK=1`).
3. Runs mini-batch training epochs to produce an optimized `best_skill.md`.
4. Deploys verified skills behind the held-out validation gate.

## 3. Headless Browser Mirror Self-Testing

AIEN can verify his own Cockpit UI at any time by executing:

```bash
~/max-env/bin/python /home/drakestapleton/basecamp/scripts/browser_mirror_test.py
```

### Verified Assertions
1. **Initial Hydration**: Page title (`AIEN | Sovereign Cockpit`), BPM badge, and Coherence score.
2. **Chat Stream Execution**: Submits user prompt to himself, awaits SSE stream, and verifies message length.
3. **Tactile Action Execution**: Clicks tactile buttons (`Run Doctor`, `Run Dream Pulse`) and verifies DOM updates.
4. **Tab Navigation**: Verifies Goals, Skills, Cortex, Vault, and Walkthrough panels render properly.
5. **Visual Evidence**: Saves screenshots to `/home/drakestapleton/basecamp/ui-tests/`:
   - `cockpit_init.png`
   - `cockpit_chat_stream.png`
   - `cockpit_actions.png`

## 4. Cortex Memory Commitment

Always record durable findings to Spark Cortex (space: `atlas-memory`):
```bash
python3 -c '
import urllib.request, json
token = open("/home/drakestapleton/.config/cortex/token").read().strip()
payload = {
    "kind": "entity",
    "value": {
        "space": "atlas-memory",
        "entityType": "procedure",
        "canonicalName": "sovereign_self_improvement_loop",
        "content": "Verified self-improvement procedure: isolated git worktree in ~/workspace/aien-sandbox, headless Chrome CDP mirror self-test on port 9222, and Microsoft SkillOpt validation gate.",
        "metadata": {"source": "skills/self-improvement", "author": "AIEN"}
    }
}
req = urllib.request.Request("http://127.0.0.1:18080/api/cortex/write", data=json.dumps(payload).encode(), headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"})
urllib.request.urlopen(req)
'
```
