# Qwen3-8B chat template (pinned)

- Source: Hugging Face `Qwen/Qwen3-8B` revision b968826d9c46dd6066d109eabc6255188de91218,
  `tokenizer_config.json` field `chat_template`, written byte for byte (no trailing newline) to `chat_template.jinja`.
- The upstream file was fetched at that exact revision
  (`https://huggingface.co/Qwen/Qwen3-8B/raw/b968826d9c46dd6066d109eabc6255188de91218/tokenizer_config.json`)
  and is byte-identical to `~/models/qwen3-8b/tokenizer_config.json` (the weights lane's download).
- Licence: Apache-2.0, upstream `LICENSE` copied unchanged (identical to the Qwen3-8B model folder's LICENSE). Not relicensed.
- sha256 chat_template.jinja: a55ee1b1660128b7098723e0abcd92caa0788061051c62d51cbe87d9cf1974d8
- sha256 upstream tokenizer_config.json: d5d09f07b48c3086c508b30d1c9114bd1189145b74e982a265350c923acd8101
- `render_oracle.json`: the template rendered by the reference engine, jinja2 3.1.6
  `ImmutableSandboxedEnvironment(trim_blocks=True, lstrip_blocks=True)` (the Hugging Face transformers configuration),
  `tools=None`, `add_generation_prompt=True`, `enable_thinking=False`, 10 conversations. Produced once with a scratch
  script outside this repository (no Python in the repository); the Rust test compares the engine's rendering to it.
- The engine renders only the `enable_thinking=false` form (`ChatTemplate::ChatMlQwen3NoThink`); other layouts are refused.
