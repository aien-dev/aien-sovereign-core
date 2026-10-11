# Evaluation of Qwen/Qwen3-8B as the next public-model candidate (D1)

Status: EVALUATION ONLY. Nothing here promotes the model. Owning issue: aien-dev/aien-sovereign-core#384 (Drake ruling 2026-10-11,
aien-architecture#190 comment 6109731585, sc#384 comment 6109731974: Qwen3-4B-Instruct-2507 is retired after the frozen FAILs v5 and v6).
Written 2026-10-11 on the sovereign-core main at 5886f91. No GPU use was intended; one slip is disclosed in Section 7.

Tags: OBSERVED = read from a file or command output on this machine today. SOURCE(url) = taken from the cited page. UNVERIFIED = reasoning or
estimate, with a confidence. Revision pinned for every Hugging Face fact: `b968826d9c46dd6066d109eabc6255188de91218`
(SOURCE https://huggingface.co/api/models/Qwen/Qwen3-8B, lastModified 2025-07-26).

## 0. Verdict

```text
engine compatibility = SUPPORTED (updated 2026-10-11 16:10Z): template pinned sc#436, SMOKE-8B-v1 PASS attempt 2 (GB10 ids equal CPU golden, fallback 0).
license              = Apache-2.0, redistribution allowed
size                 = 16.38 GB of bf16 weights in 5 shards (2.04 times the 4B)
weights on disk      = ~/models/qwen3-8b, all five shards re-hashed and equal to Hugging Face's own sha256 (Section 1)
next step            = v7 campaign driver with ALLEN engaged + complete dry-run gate, then freeze v7 (334 rows unchanged)
```

## 1. Provenance and license

- Publisher repo `Qwen/Qwen3-8B`, revision b968826d9c46dd6066d109eabc6255188de91218. SOURCE(https://huggingface.co/api/models/Qwen/Qwen3-8B).
- License: Apache-2.0. SOURCE(https://huggingface.co/Qwen/Qwen3-8B: "License: apache-2.0"; API license tag apache-2.0). OBSERVED: the repo's `LICENSE`
  file has sha256 832dd9e00a68dd83b3c3fb9f5588dad7dcf337a0db50f7d9483f310cd292e92e, byte-identical to the 4B's LICENSE that the repo already carries
  (`crates/aien-inference-abi/fixtures/chat-templates/qwen3-4b-instruct-2507/PROVENANCE.md`).
- Parameters: 8.2 B total, 6.95 B non-embedding. SOURCE(https://huggingface.co/Qwen/Qwen3-8B). The safetensors header counts 8,190,735,360 parameters, all BF16. SOURCE(API above).
- Files downloaded (OBSERVED, `huggingface`-style `resolve/<revision>` URLs, detached `curl`, log `~/models/logs/dl-qwen3-8b.log`, hashes `~/models/logs/qwen3-8b.sha256`):

```text
model.safetensors.index.json  f9fdbcb91c23971c13ec5d5f2573d2349e8f61f2f049371ec699281748fdb1bc   (total_size 16,381,470,720 bytes)
model-00001-of-00005          31d6a825ae35f11fb85b195b4c42c146c051e446433125a215336abdf95cbf5f   3,996,250,744
model-00002-of-00005          5991236cea6fe21f3d43cab0f0e84448734fbbe0789816202989f2ddc9d18282   3,993,160,032
model-00003-of-00005          c5185c4794be2d8a9784d5753c9922db38df478ce11f9ed0b415b7304d896836   3,959,604,768
model-00004-of-00005          b5ee7de71fbf17db3d5704e0c8f2bc7d005ca9e1d7ca2aeb19827b0cfcaa917a   3,187,841,392
model-00005-of-00005          20c2d6366ab85c90786ccdd829cd2b9e7d30ef3b2ebbb998280e7e4014b542ff   1,244,659,840
config.json                   f7c4eadfbbf522470667b797a3c89be2524832d2d599797248dc304fff447c30
generation_config.json        2325da0f15bb848e018c5ae071b7943332e9f871d6b60e2ed22ca97d4cb993d2
tokenizer_config.json         d5d09f07b48c3086c508b30d1c9114bd1189145b74e982a265350c923acd8101
tokenizer.json                aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4
```

- The five shard hashes and tokenizer.json equal the LFS object ids Hugging Face lists for that revision. SOURCE(https://huggingface.co/api/models/Qwen/Qwen3-8B/tree/b968826d9c46dd6066d109eabc6255188de91218). The index, config files and LICENSE are small non-LFS files with no published oid, so they are OBSERVED only.
- The shards add up to the index total plus 46,056 bytes of safetensors headers. OBSERVED (arithmetic on the sizes above).
- The weights have not been loaded by the engine. The `model_sha256` that the daemon prints (index plus shards manifest, DAEMON_GENERATION_RECORD.md) is therefore not yet computed: UNKNOWN, filled at freeze.

## 2. Architecture against the 4B

OBSERVED from `~/models/qwen3-4b-instruct-2507-cdbee75/config.json` and the 8B `config.json` above. The two files carry the same set of keys.

| field | 4B-Instruct-2507 | 8B | engine effect |
|---|---|---|---|
| architectures / model_type | Qwen3ForCausalLM / qwen3 | same | accepted (`model_dir.rs` QWEN3_ARCHITECTURE) |
| num_hidden_layers | 36 | 36 | same |
| hidden_size | 2560 | 4096 | all matmul shapes change; derived from the config, not hard-coded (SOURCE `gb10_serving.rs` `serving_matmul_shapes`, `gb10_serving_bounds`) |
| intermediate_size | 9728 | 12288 | widest GB10 matmul input becomes 12288 (was 9728); derived (`max_k = max(hidden, q_dim, intermediate)`) |
| num_attention_heads / num_key_value_heads | 32 / 8 | 32 / 8 | same |
| head_dim | 128 | 128 | same; here q_dim 4096 equals hidden_size, for the 4B it was 4096 against 2560 |
| vocab_size | 151936 | 151936 | same (logit width 151936) |
| tie_word_embeddings | true | **false** | the 8B ships a separate `lm_head.weight` (OBSERVED in the index). The loader supports an untied head (SOURCE `weights.rs` lines 757-759, 822-825; `resident.rs` line 249). Not exercised on the Qwen3 GB10 path before: UNVERIFIED, see Section 8. |
| rope_theta | 5,000,000 | 1,000,000 | read from config; value changes, mechanism does not |
| rope_scaling | null | null | same |
| max_position_embeddings | 262144 | 40960 | the product caps the context at 4096 (`AIEN_KV_CONTEXT_TOKENS`), below both |
| rms_norm_eps, hidden_act, bias flags, sliding window | 1e-6, silu, no bias, off | same | same |

- Every `config.json` key of the 8B is in the engine's Qwen3 known-keys list, because the 4B carries the same keys. OBSERVED (key sets equal) plus SOURCE(`model_dir.rs` `QWEN3_KNOWN_KEYS`).
- Per layer the tensor names are the same eleven as the 4B, including `q_norm` and `k_norm`. OBSERVED (8B index, layer 0).
- The 8B is one dense model, not mixture-of-experts. OBSERVED (no expert tensors in the index).

## 3. Tokenizer

- `tokenizer.json` of the 8B has sha256 aeb13307a71acd...4dae4, byte-identical to the 4B's. OBSERVED. `vocab.json` is also identical (ca10d7e9...). `merges.txt` differs between the folders (8831e4f1... against 599bab54...); the engine reads `tokenizer.json`, so UNVERIFIED but low risk (95 percent) that this does not matter.
- `tokenizer_config.json` differs from the 4B's only in values outside the chat template that the engine does not read for chat rendering: `model_max_length` 131072 against 1010000. OBSERVED. eos `<|im_end|>`, pad `<|endoftext|>`, `add_bos_token` false in the 8B. SOURCE(https://huggingface.co/Qwen/Qwen3-8B/raw/main/tokenizer_config.json).
- Stop ids: 8B `generation_config.json` has eos_token_id [151645, 151643]. OBSERVED. The 4B daemon logs stop ids [151645, 151643] (OBSERVED in the proof run daemon log, Section 7), so the stop set is unchanged.
- `generation_config.json` of the 8B carries sampling defaults (temperature 0.6, top_k 20, top_p 0.95). OBSERVED. The campaigns decode greedily.

## 4. Chat template and thinking mode

- The 8B template is NOT the 4B-2507 template. OBSERVED (`diff` against the pinned `chat_template.jinja`): it adds (a) a `last_query_index` scan, (b) `reasoning_content` and `<think>` handling for assistant turns, and (c) at the generation prompt, `<think>\n\n</think>\n\n` when `enable_thinking` is false. 4169 characters against the 4B's 2507-pinned text.
- Thinking is ON by default. SOURCE(https://huggingface.co/Qwen/Qwen3-8B: "By default, Qwen3 has thinking capabilities enabled.") Hard switch: `enable_thinking=False` in the template call. Soft switch `/no_think` in a message works only when thinking is enabled. Model card sampling advice: thinking T=0.6 TopP=0.95 TopK=20 and "DO NOT use greedy decoding"; non-thinking T=0.7 TopP=0.8 TopK=20. SOURCE(same page).
- The engine refuses it today: `parse_chatml` accepts exactly the pinned 4B-2507 text, the two plain ChatML layouts, or one plain-text default system message, and returns "unsupported ChatML chat template" for anything else. SOURCE(`crates/aien-inference-abi/src/tokenizer.rs`, `parse_chatml`). The engine also has no `enable_thinking` input at all (OBSERVED: no `think` in `tokenizer.rs`). So the daemon would fail at model load, before any token.
- How the product must disable thinking: render with `enable_thinking=false`, which for a conversation without tools and without prior assistant thinking is exactly the plain ChatML turns followed by `<|im_start|>assistant\n<think>\n\n</think>\n\n`. UNVERIFIED (derived by reading the template text; confidence 90 percent for the no-tools, no-prior-assistant case the campaigns use). The prior-assistant-turn branch (`reasoning_content`, `last_query_index`) is not needed by the campaign prompts (single user turn plus optional system turn, see the task files) but the tool-response branch is identical to the 4B's.

## 5. Engine compatibility verdict: NEEDS WORK (at writing; CLOSED 2026-10-11, see the update under each gap)

Gaps, each with the code place:

1. **Chat template (blocks the daemon).** `tokenizer.rs` `parse_chatml` and the `ChatTemplate` enum need a second pinned Qwen layout, for example `ChatMlQwen3Hybrid` (name open), that (a) pins the 8B template text byte for byte as a fixture next to the 4B's with a `PROVENANCE.md` and a render oracle, (b) always appends `<think>\n\n</think>\n\n` after the generation prompt (thinking off as a fixed product setting, no per-call switch), and (c) refuses prior assistant turns that carry `<think>` blocks rather than approximating. Tests: extend `tests/qwen3_chat_template.rs` and `tests/tool_boundary.rs`. Also `spine.rs` line 1151 matches `ChatMlQwen3` by name for the "filename:" prompt label and needs the new variant.
2. **Untied output head on the qualified GB10 path (OBSERVED fine 2026-10-11: SMOKE-8B-v1 attempt 2 PASS, GB10 ids equal CPU golden on both prompts).** `weights.rs`/`resident.rs` handle `lm_head.weight` generally; the Qwen3 parity and GB10 tests were written with the tied 4B. Needs the smoke test of Section 8 to turn this into OBSERVED either way. Extra resident bytes if streamed: vocab 151936 x 4096 x 2 = 1.24 GB (arithmetic).
3. **Sampling mismatch (not a code gap).** The model card advises against greedy decoding; the campaigns are greedy. This can only be measured by running, and cannot be a reason to change a threshold.

Not gaps (OBSERVED or SOURCE): `Qwen3ForCausalLM` accepted; all config keys known; 36 layers, 8 KV heads, head_dim 128 same as the 4B; GB10 serving bounds computed from the config (`gb10_serving_bounds`); safetensors sharded checkpoints with an index are the engine's format (the 4B is loaded that way, three shards, OBSERVED in its folder and in the v6 evidence).

## 6. Resource estimate

- Weights: 16,381,470,720 bytes bf16 (OBSERVED, index) against 8,044,982,000 for the 4B (OBSERVED, shard sizes): 2.04 times. Fits the 128 GB unified memory with large margin. UNVERIFIED how the engine holds them (bf16 resident, or converted); the 4B daemon's `VmHWM` per launch is in the v6 evidence and the same measure applies. Row `MEAS` records memory.
- KV cache at the product's 4096-token context: per token 36 layers x 8 KV heads x 128 x 2 (K and V) = 73,728 values. At 2 bytes per value that is 147,456 bytes per token, 0.60 GB for 4096 tokens; at 4 bytes 1.21 GB. Identical to the 4B because layers, KV heads and head_dim are equal (OBSERVED configs). Storage width in the pooled KV is UNVERIFIED.
- Latency: decode on a memory-bandwidth-bound chip scales with bytes read per token. The 4B ran D1 at 542 tokens in 51,968 ms, about 10.4 tokens per second (OBSERVED, RESULT-v6.md Section 2). A 2.04 times larger weight set predicts roughly 5 tokens per second for the 8B. UNVERIFIED, confidence 60 percent (±30 percent; prefill and kernel efficiency may differ).
- Consequence for the unchanged budgets (OBSERVED in `tasks-oq3-v5.json`): document tasks have 120,000 ms and D6 170,000 ms for up to two attempts. At about 5 tokens per second one 500-token attempt costs about 100 s, so a second attempt will often hit the budget (class (d), as D6 did for the 4B). This is a risk to the pass rate, not a reason to touch a threshold or a budget.
- Warm-up and start time grow with weight size; the S7 restart reloads 16 GB. UNVERIFIED, estimate 2 times the 4B's 27.6 s restart (OBSERVED, dry D3 step S7).

## 7. Disk, download and a disclosed slip

- Disk before download: 454 GB free (OBSERVED `df -h ~`). Format confirmed safetensors. Download of the five shards finished; log shows `DONE`.
- Slip (against this lane's instruction not to use the GPU): to prove the ALLEN fix I ran the new driver on the 4B with `AIEN_FORCE_CPU_STUB=1`,
  expecting the CPU path. That flag only stubs the GPU helper library; the daemon still chose the GB10 backend (OBSERVED, `evidence-v7-d1/after-fix-containment.txt`:
  "Backend: NativeTransformerBackend/OmegaGb10Backend ... NVIDIA GB10 sm_121"). No quiet hold was taken. The run lasted about 11 minutes (two daemon
  starts of about 27 s each and one S3 attempt that timed out at 590,041 ms with 0 tokens; the cause was not investigated: this run had no
  `AIEN_GB10_QWEN3_DECLARED_ATTEMPT=1` and used a 1024-token KV, unlike a campaign run). It exited cleanly (driver exit 0, the driver's own shutdown
  call stopped the daemon, no daemon process left, `nvidia-smi` shows no compute process afterwards). Nothing was killed. Anyone reading
  chip timings from that window should discount them.
- What the proof run establishes (OBSERVED, `evidence-v7-d1/`): with the new driver the daemon logged `ALLEN: engaged` (fingerprint 94bb94be), S1 "remember"
  returned ok with constraint id 11 (the step v6 had refused), S2 ok, S7 restart ok (subject named, not re-adopted), S8 recall ok.
  It also exposed a second defect, fixed in the same driver: S1 writes into `compose.allen-memory/`, which the unchanged containment rows read as an
  unexpected file (they demand `outside_new_files == ["./compose.cortex-mark"]`). The driver now lists the ALLEN state files under `run.json` `.allen.state_files`
  instead of in `outside_new_files`; the same find re-run on the proof folder returns only `./compose.cortex-mark`. The proof's `run.json` is from before that
  exclusion (it still lists the ALLEN files in `outside_new_files`); the after-fix output is in `after-fix-containment.txt`. This is a measurement-scope
  decision, not a row or threshold change; reviewers should confirm it.
- What it does not establish: a full launch (S3 produced no tokens), the receipt rows, or any 8B behaviour.

## 8. Plan for the correctness smoke test (run only after the template gap is closed)

Goal: show that the 8B produces correct tokens on the native engine, with the evidence kinds of the 4B work (`examples/qwen3_parity_check.rs`, `tests/qwen3_forward_parity.rs`, R1 reference leg of the campaign).

1. **Tokenizer ids.** Encode 3 fixed strings (an ASCII sentence, a code line, a multi-byte string) with the engine and compare to ids produced once by the Hugging Face tokenizer for the same `tokenizer.json`. Since `tokenizer.json` equals the 4B's, the existing tokenizer tests already cover it: only the template differs.
2. **Template render.** The golden conversation `[system "You are a careful assistant.", user "Say OK."]` must render to `<|im_start|>system\nYou are a careful assistant.<|im_end|>\n<|im_start|>user\nSay OK.<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n` (OBSERVED 2026-10-11: render_oracle.json produced by jinja2 3.1.6 off-repo from the publisher template, 10/10 cases byte-equal to the engine in `qwen3_8b_chat_template.rs`; re-rendered independently from ~/models/qwen3-8b/tokenizer_config.json the same day, 10/10 equal).
3. **Golden prompt to ids.** Off-repo, once, with the Hugging Face reference (scratch Python allowed outside the repo, `enable_thinking=False`, greedy, 16 new tokens, bf16) record the 16 token ids for the prompt "Reply with the single word OK." Commit them as the expected ids.
4. **Engine CPU reference.** Run the CPU reference leg (`np1_reference` style, no GPU) for the same 16 tokens: expected equal to step 3 token for token or equal up to a bf16 near-tie, which is then reported with the logit margin (as `qwen3_noise_floor.rs` does for the 4B). Do not accept a mismatch by raising a tolerance.
5. **GB10 leg, quiet hold only.** Same prompt on the GB10 backend: tokens must equal the CPU reference leg (the R1 identity rule). Record `VmHWM`, warm-up ms, decode ms per token.
6. **Untied head check.** Step 4 and 5 are the check for gap 2; an untied-head failure shows as logits from the wrong matrix (all tokens wrong), not as a small drift.

Smoke PASS means steps 2 to 5 pass. It is not a qualification and not a promotion.

## 9. Limits of this document

- No engine run of the 8B happened (the weights were downloaded and hashed only). Every compatibility statement about runtime behaviour is derived from reading code and configs.
- Hugging Face page facts came through a page summariser; config values were then read from the actual downloaded files (OBSERVED), and the template differences by `diff` on the file.
