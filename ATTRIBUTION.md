# Open-Source Attribution & Acknowledgments

## Modular (MAX Engine & Mojo)

This project builds upon, interfaces with, and contributes to the revolutionary AI infrastructure pioneered by **Modular Inc.** (https://modular.com).

- **MAX Engine & Mojo Language**: We utilize Modular MAX Engine for high-performance tensor execution and the Mojo programming language for compiled, zero-overhead neural graph kernels.
- **Upstream Stewardship**: In accordance with our Drop != Delete protocol, any custom architectures, KV-cache expansions, and low-latency C-ABI bridges engineered on our hardware are contributed directly back upstream to the open-source Modular ecosystem.
- **License Notice**: Modular MAX, Mojo, and related components are subject to their respective licenses by Modular Inc. (including Apache License 2.0 with LLVM Exceptions and Modular Community Licenses). All trademarks, service marks, and trade names of Modular Inc. belong to Modular Inc.

We extend our deep gratitude to Chris Lattner, Tim Davis, and the entire Modular engineering team for their vision in unifying AI hardware and software through high-performance compiled native systems.

---

## Additional Upstream Foundations

- **NVIDIA**: Blackwell GB10 architecture, TensorRT, NeMo/Nemotron models, and CUDA driver runtime.
- **The Rust Community**: Tokio, Axum, Ort (ONNX Runtime C-API bindings), and HuggingFace Tokenizers.
- **Radicle**: Sovereign peer-to-peer code collaboration and Heartwood git protocol.

---

## Third-Party Model: SmolLM2-1.7B-Instruct (Hugging Face)

The runtime can load and run **SmolLM2-1.7B-Instruct** by Hugging Face (HuggingFaceTB),
an open-weights instruction model. It is a separate third-party component, not part of
the AIEN source code, and it keeps its own licence.

- **Source**: https://huggingface.co/HuggingFaceTB/SmolLM2-1.7B-Instruct, revision
  `31b70e2e869a7173562077fd711b654946d38674`. Weights `model.safetensors` sha256
  `f55217be716b6a997b97b9d8d7eb6fad02e00858f5010ec24f64603c3a98a0e8` (equal to the
  publisher's Git LFS object id). Used unmodified: no conversion, no quantization.
- **Licence**: Apache License 2.0 (`Apache-2.0`), as declared in the model card
  (`license: apache-2.0`). Full text: `LICENSES/Apache-2.0.txt`. The repository at this
  revision ships no separate LICENSE or NOTICE file; its model card (`README.md`) is the
  publisher's attribution and is distributed unchanged with the weights.
- **Attribution** (the model card's citation): Loubna Ben Allal, Anton Lozhkov, Elie
  Bakouch, Gabriel Martín Blázquez, Guilherme Penedo, Lewis Tunstall, Andrés Marafioti,
  Hynek Kydlíček, Agustín Piqueres Lajarín, Vaibhav Srivastav, Joshua Lochner, Caleb
  Fahlgren, Xuan-Son Nguyen, Clémentine Fourrier, Ben Burtenshaw, Hugo Larcher, Haojun
  Zhao, Cyril Zakka, Mathieu Morlon, Colin Raffel, Leandro von Werra, Thomas Wolf,
  "SmolLM2: When Smol Goes Big -- Data-Centric Training of a Small Language Model",
  arXiv:2502.02737 (2025).
- **Boundary**: AIEN code stays under AGPL-3.0-or-later. The model files stay under
  Apache-2.0 with their notices preserved. Running the model through AIEN, or storing it
  under another directory or file name, does not change either licence.
