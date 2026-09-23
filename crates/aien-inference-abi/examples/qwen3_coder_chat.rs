//! Native Qwen3-Coder-30B-A3B chat: full 48-layer forward, no MAX, no Python.
//!
//! Usage:
//!   qwen3_coder_chat [checkpoint_dir] [--max-new 256] [--device serve]
//!
//! Lines on stdin are wrapped in the Qwen3 ChatML template and decoded
//! greedily. Default is CPU; `--device serve` routes every GEMM through the
//! resident-weight serve library. Type `quit` to exit.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::time::Instant;

use aien_inference_abi::qwen3_coder::{
    compute_logits, forward_token, load_qwen3_coder, Qwen3CoderState, Qwen3CoderWeights,
};
use aien_inference_abi::qwen3_serve::{forward_token_serve, QwenServeLib, QwenServeModel};
use aien_inference_abi::tensor::sample_argmax;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut checkpoint: Option<PathBuf> = None;
    let mut max_new: usize = 256;
    let mut device = false;
    while let Some(arg) = args.next() {
        if arg == "--max-new" {
            max_new = args
                .next()
                .expect("--max-new needs a value")
                .parse()
                .expect("--max-new must be a number");
        } else if arg == "--device" {
            let kind = args.next().expect("--device needs a value");
            if kind == "serve" {
                device = true;
            } else {
                eprintln!("unknown --device {kind}");
                std::process::exit(2);
            }
        } else if checkpoint.is_none() {
            checkpoint = Some(PathBuf::from(arg));
        } else {
            eprintln!("unexpected argument: {arg}");
            std::process::exit(2);
        }
    }
    let checkpoint = checkpoint.unwrap_or_else(|| {
        PathBuf::from("/home/drakestapleton/.cache/huggingface/hub/models--Qwen--Qwen3-Coder-30B-A3B-Instruct-FP8/snapshots/dcaee4d4dfc5ee71ad501f01f530e5652438fde0")
    });

    let t0 = Instant::now();
    let weights = load_qwen3_coder(&checkpoint).expect("load Qwen3-Coder weights");
    eprintln!("weights loaded in {:.1}s", t0.elapsed().as_secs_f32());

    let serve = if device {
        let t0 = Instant::now();
        let lib = QwenServeLib::load(&QwenServeLib::default_path()).expect("load serve .so");
        let serve = QwenServeModel::upload_all(lib, &weights, &checkpoint).expect("upload weights");
        eprintln!("device upload in {:.1}s", t0.elapsed().as_secs_f32());
        Some(serve)
    } else {
        None
    };

    let tokenizer = tokenizers::Tokenizer::from_file(checkpoint.join("tokenizer.json"))
        .expect("load tokenizer.json");
    let stop_ids: Vec<u32> = ["<|im_end|>", "<|endoftext|>"]
        .iter()
        .filter_map(|s| {
            tokenizer
                .encode(*s, false)
                .ok()
                .and_then(|e| e.get_ids().first().copied())
        })
        .collect();
    eprintln!("stop ids: {stop_ids:?}");

    let stdin = io::stdin();
    print!("eien> ");
    io::stdout().flush().unwrap();
    for line in stdin.lock().lines() {
        let line = line.expect("read stdin line");
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("quit") || trimmed.eq_ignore_ascii_case("exit") {
            break;
        }
        if trimmed.is_empty() {
            print!("eien> ");
            io::stdout().flush().unwrap();
            continue;
        }
        let prompt = format!("<|im_start|>user\n{trimmed}<|im_end|>\n<|im_start|>assistant\n");
        let prompt_ids: Vec<u32> = tokenizer
            .encode(prompt, false)
            .expect("encode prompt")
            .get_ids()
            .to_vec();

        let mut state = Qwen3CoderState::new();
        let forward = |weights: &Qwen3CoderWeights,
                       serve: &Option<QwenServeModel>,
                       state: &mut Qwen3CoderState,
                       tok: u32,
                       pos: usize|
         -> Vec<f32> {
            match serve {
                Some(s) => {
                    forward_token_serve(weights, s, tok, pos, state, None, false).expect("device forward")
                }
                None => forward_token(weights, tok, pos, state),
            }
        };
        let logits =
            |weights: &Qwen3CoderWeights, serve: &Option<QwenServeModel>, hidden: &[f32]| -> u32 {
                let v = match serve {
                    Some(s) => {
                        let mut out = vec![0.0f32; 151936];
                        s.logits(hidden, &mut out).expect("device logits");
                        out
                    }
                    None => compute_logits(weights, hidden),
                };
                sample_argmax(&v).0
            };
        let t_pre = Instant::now();
        for (pos, &tok) in prompt_ids.iter().enumerate() {
            forward(&weights, &serve, &mut state, tok, pos);
        }
        let prefill_s = t_pre.elapsed().as_secs_f32();
        let prompt_len = prompt_ids.len();
        let mut prev = *prompt_ids.last().unwrap();
        let mut generated: Vec<u32> = Vec::new();
        let t_dec = Instant::now();
        let mut printed_len = 0usize;
        for step in 0..max_new {
            let hidden = forward(&weights, &serve, &mut state, prev, prompt_len + step);
            let next = logits(&weights, &serve, &hidden);
            if stop_ids.contains(&next) {
                break;
            }
            generated.push(next);
            prev = next;
            let text = tokenizer.decode(&generated, true).unwrap_or_default();
            if text.len() > printed_len {
                print!("{}", &text[printed_len..]);
                io::stdout().flush().unwrap();
                printed_len = text.len();
            }
        }
        let decode_s = t_dec.elapsed().as_secs_f32();
        println!();
        eprintln!(
            "[prefill {:.1} tok/s | decode {:.2} tok/s | {} tokens]",
            prompt_ids.len() as f32 / prefill_s.max(1e-6),
            generated.len() as f32 / decode_s.max(1e-6),
            generated.len()
        );
        print!("eien> ");
        io::stdout().flush().unwrap();
    }
}
