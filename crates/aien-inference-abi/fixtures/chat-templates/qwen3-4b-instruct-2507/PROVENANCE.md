# Qwen3-4B-Instruct-2507 chat template (pinned)

- Source: Hugging Face `Qwen/Qwen3-4B-Instruct-2507` revision cdbee75f17c01a7cc42f958dc650907174af0554,
  `tokenizer_config.json` field `chat_template`, written byte for byte (no trailing newline) to `chat_template.jinja`.
- Licence: Apache-2.0, upstream `LICENSE` copied unchanged into this folder. Not relicensed.
- sha256 chat_template.jinja: 64f85b198065d0fba2a81f37e10ed68161ce2c19a754c7100e67e0ca2ee9c326
- sha256 upstream tokenizer_config.json: a62ff0a2472a0fa1b8eaabcb57c59b58afa42a22831dc141400b6e0cf2b65ce3
- sha256 LICENSE: 832dd9e00a68dd83b3c3fb9f5588dad7dcf337a0db50f7d9483f310cd292e92e
- `render_oracle.json`: the template rendered by the reference engine, jinja2 3.1.6 `ImmutableSandboxedEnvironment(trim_blocks=True, lstrip_blocks=True)`
  (the Hugging Face transformers configuration), `tools=None`, 8 conversations. Produced once with a scratch
  script outside this repository (no Python in the repository); the Rust test compares the engine's rendering to it.
