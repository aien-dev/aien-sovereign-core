//! `validate_record` refuses every corruption with the named error and without
//! touching the heap (ADR 0003 section 1.4, CUTS C1 red test
//! `validate_refuses_before_any_allocation`). This file is its own test binary
//! so its counting global allocator sees only this binary.

mod common;

use aien_kv_store::{validate_record, ChunkHeader, KvError, RecordHeader};
use common::*;
use sha2::{Digest, Sha256};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAlloc;

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
}

fn bump() {
    ARMED.with(|a| {
        if a.get() {
            COUNT.with(|c| c.set(c.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        bump();
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        bump();
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        bump();
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

/// Run `f` and return its result with the number of heap allocations it made.
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|c| c.set(0));
    ARMED.with(|a| a.set(true));
    let out = f();
    ARMED.with(|a| a.set(false));
    (out, COUNT.with(|c| c.get()))
}

fn reseal(b: &mut [u8; 256]) {
    let d = Sha256::digest(&b[..224]);
    b[224..].copy_from_slice(&d);
}

fn put32(b: &mut [u8; 256], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

struct Fixture {
    expect: ChunkHeader,
    bytes: [u8; 256],
    file_size: u64,
}

fn fixture() -> Fixture {
    let expect = sample_chunk_header();
    let bytes = RecordHeader {
        chunk: expect,
        payload_sha256: [7; 32],
    }
    .encode();
    let file_size = 256 + expect.payload_len;
    Fixture {
        expect,
        bytes,
        file_size,
    }
}

#[test]
fn counter_detects_allocation() {
    // Negative control: the harness would notice a heap use.
    let (_, n) = counted(|| std::hint::black_box(Vec::<u8>::with_capacity(64)));
    assert!(n >= 1, "counting allocator is not counting");
}

#[test]
fn valid_record_passes_with_zero_allocations() {
    let f = fixture();
    let (r, n) = counted(|| validate_record(&f.bytes, f.file_size, &f.expect));
    assert_eq!(n, 0);
    let r = r.expect("valid record");
    assert_eq!(r.chunk, f.expect);
    assert_eq!(r.payload_sha256, [7; 32]);
}

#[test]
fn validate_refuses_before_any_allocation() {
    type Corrupt = (&'static str, KvError, fn(&mut Fixture));
    let cases: Vec<Corrupt> = vec![
        ("wrong magic", KvError::BadMagic, |f| f.bytes[0] = b'X'),
        ("wrong version", KvError::UnsupportedVersion, |f| {
            f.bytes[8] = 2;
            reseal(&mut f.bytes);
        }),
        ("wrong header hash", KvError::HeaderChecksum, |f| {
            f.bytes[230] ^= 1;
        }),
        ("wrong namespace", KvError::NamespaceMismatch, |f| {
            f.bytes[16] ^= 1;
            reseal(&mut f.bytes);
        }),
        ("wrong key", KvError::KeyMismatch, |f| {
            f.bytes[48] ^= 1;
            reseal(&mut f.bytes);
        }),
        ("wrong parent", KvError::ParentMismatch, |f| {
            f.bytes[80] ^= 1;
            reseal(&mut f.bytes);
        }),
        ("wrong position", KvError::PositionMismatch, |f| {
            put32(&mut f.bytes, 144, 256);
            reseal(&mut f.bytes);
        }),
        ("token_count 0", KvError::TokenCountOutOfRange, |f| {
            put32(&mut f.bytes, 148, 0);
            reseal(&mut f.bytes);
        }),
        (
            "token_count over chunk_tokens",
            KvError::TokenCountOutOfRange,
            |f| {
                put32(&mut f.bytes, 148, 257);
                reseal(&mut f.bytes);
            },
        ),
        ("wrong geometry", KvError::GeometryMismatch, |f| {
            put32(&mut f.bytes, 164, 9); // num_kv_heads
            reseal(&mut f.bytes);
        }),
        ("wrong payload_len", KvError::PayloadLenMismatch, |f| {
            f.bytes[176] ^= 1;
            reseal(&mut f.bytes);
        }),
        (
            "file shorter than header+payload",
            KvError::FileSizeMismatch,
            |f| {
                f.file_size -= 1;
            },
        ),
        (
            "file longer than header+payload",
            KvError::FileSizeMismatch,
            |f| {
                f.file_size += 1;
            },
        ),
        ("truncated header", KvError::Truncated, |_| {}),
    ];
    assert!(cases.len() >= 12);
    for (name, want, corrupt) in cases {
        let mut f = fixture();
        corrupt(&mut f);
        let slice: &[u8] = if want == KvError::Truncated {
            &f.bytes[..200]
        } else {
            &f.bytes
        };
        let (got, allocs) = counted(|| validate_record(slice, f.file_size, &f.expect));
        assert_eq!(got, Err(want), "{name}");
        assert_eq!(allocs, 0, "{name}: validation allocated");
    }
}

#[test]
fn validation_reports_the_first_failure_in_adr_order() {
    // Magic and namespace both wrong: magic wins.
    let mut f = fixture();
    f.bytes[0] = b'X';
    f.bytes[16] ^= 1;
    reseal(&mut f.bytes);
    assert_eq!(
        validate_record(&f.bytes, f.file_size, &f.expect),
        Err(KvError::BadMagic)
    );
    // Namespace and key both wrong (hash valid): namespace wins.
    let mut f = fixture();
    f.bytes[16] ^= 1;
    f.bytes[48] ^= 1;
    reseal(&mut f.bytes);
    assert_eq!(
        validate_record(&f.bytes, f.file_size, &f.expect),
        Err(KvError::NamespaceMismatch)
    );
}

#[test]
fn partial_flag_must_agree_with_token_count() {
    // A header claiming partial_tail on a full chunk is refused.
    let mut f = fixture();
    f.bytes[174] = 1;
    reseal(&mut f.bytes);
    assert_eq!(
        validate_record(&f.bytes, f.file_size, &f.expect),
        Err(KvError::BadFlags)
    );
    // A reserved flag bit is refused.
    let mut f = fixture();
    f.bytes[175] = 0x80;
    reseal(&mut f.bytes);
    assert_eq!(
        validate_record(&f.bytes, f.file_size, &f.expect),
        Err(KvError::BadFlags)
    );
}
