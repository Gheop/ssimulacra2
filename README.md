# ssimulacra2 (fast fork)

Rust implementation of the [SSIMULACRA2 metric](https://github.com/cloudinary/ssimulacra2).

This is a performance fork of [rust-av/ssimulacra2](https://github.com/rust-av/ssimulacra2),
built for quality-target search loops (find the smallest encoder quality whose score still
meets a target), where the same reference image is scored against many candidates. Scores
stay within 1e-4 of upstream; in practice they are identical to the 8th decimal
(`ssimulacra2/tests/conformance.rs`).

What it adds over upstream:

- **`ReferenceFrame`**: precompute the reference pyramid (downscale chain, XYB conversion,
  `mu1`, `sigma1_sq`) once, then score any number of candidates against it. In a search
  loop this removes ~45 % of the per-iteration work.
- **Parallel pipeline**: plane and independent blurs, SSIM/edge-diff maps, downscale and
  XYB conversion run under rayon. Per-accumulator summation order is preserved, so scores
  are bit-identical to the sequential path.
- **`ssimulacra2_rs worker`**: a persistent scoring process driven over stdin/stdout with
  raw RGB8 (or PNG) payloads. Callers in any language keep a warm reference and pay no
  process spawn, temp files, or PNG round-trip per iteration.
- The binary actually enables the lib's `rayon` feature (upstream's binary disables it
  through a `default-features = false` dependency).
- Fixes an upstream panic in lcms2 on 16-bit PNGs carrying an ICC profile.

## Benchmarks

Setup: Intel Core Ultra 7 155H (22 threads), Linux, `tank` test pair (photo), fork v0.6.0
vs upstream `ssimulacra2_rs` 0.5.2 from crates.io. CLI timings are `hyperfine` means over
10 runs (PNG decode included, `--no-icc` on the fork for identical behavior); lib timings
average 5 in-process runs.

One-shot CLI (`ssimulacra2_rs image a.png b.png`):

| Pair size | upstream 0.5.2 | this fork | speedup |
|---|---|---|---|
| 1448×1080 | 1.231 s ± 0.014 | 0.466 s ± 0.028 | 2.6× |
| 1024×764 | 0.618 s ± 0.018 | 0.228 s ± 0.016 | 2.7× |
| 640×477 | 0.232 s ± 0.013 | 0.089 s ± 0.007 | 2.6× |
| 320×239 | 0.062 s ± 0.003 | 0.026 s ± 0.002 | 2.4× |

For reference, upstream with only its existing `rayon` feature turned on (horizontal blur
pass only) reaches 1.013 s on the 1448×1080 pair: the fork's gain comes from parallelizing
the rest of the pipeline, not just from flipping the feature.

Search-loop cost per iteration, same 1448×1080 pair (what a quality search actually pays
after the reference is warm):

| Path | per-iteration cost | vs upstream one-shot |
|---|---|---|
| upstream CLI call | 1.231 s | 1.0× |
| lib, `ReferenceFrame` warm | ~0.17–0.21 s | ~6× |
| `worker` mode, raw RGB8 over stdin | 0.235 s ± 0.034 (0.119 s at 1024×764) | ~5× |
| `worker` mode, `x86-64-v3` build | 0.094 s ± 0.019 (0.053 s at 1024×764) | ~13× |

Reference construction (`ReferenceFrame::new`, paid once per reference): 176 ms at
1448×1080, 103 ms at 1024×764.

Reproduce with `cargo run --release -p ssimulacra2 --example bench_search` and
`hyperfine` on the fixtures in `ssimulacra2/test_data/`.

### Build with `target-cpu=x86-64-v3` (another ~2×)

The pipeline uses `f32::mul_add` heavily. Without FMA in the target features,
Rust lowers it to a correctly-rounded `fmaf` library call; with
`-C target-cpu=x86-64-v3` (Haswell/2013 or newer) it becomes a single FMA
instruction:

```
RUSTFLAGS="-C target-cpu=x86-64-v3" cargo install --path ssimulacra2_bin --no-default-features
```

Measured on the 1448×1080 pair: 521 ms → 268 ms (1.9×) on top of the numbers
above (same machine, same run). Caveat: FMA rounds once instead of twice, so
scores shift by less than 0.01 versus the portable build — irrelevant for
quality targeting, but run `cargo test` without the flag if you want the
goldens to match at 1e-4.

## Library usage

```rust
use ssimulacra2::{ReferenceFrame, compute_frame_ssimulacra2};

// One-shot, drop-in upstream API:
let score = compute_frame_ssimulacra2(source, distorted)?;

// Search loop: precompute the reference once.
let reference = ReferenceFrame::new(source)?;
for candidate in candidates {
    let score = reference.score(candidate)?;
}
```

## Worker mode

`ssimulacra2_rs worker` reads little-endian, length-prefixed requests on stdin and writes
one text line per request on stdout:

| Field | Type | Meaning |
|---|---|---|
| tag | u8 | `'R'` set the reference, `'S'` score a candidate |
| width | u32 LE | pixels (ignored for PNG payloads) |
| height | u32 LE | pixels (ignored for PNG payloads) |
| fmt | u8 | 0 = interleaved sRGB RGB8 (w×h×3 bytes), 1 = PNG, 2 = interleaved sRGB RGB16 LE (w×h×6 bytes) |
| len | u32 LE | payload size in bytes (max 512 MiB) |
| payload | bytes | pixels or PNG file |

Replies: `OK` (reference loaded), `SCORE <f64>`, or `ERR <message>` (session continues).
EOF on stdin exits 0; a protocol error (unknown tag, oversized length, truncated stream)
exits 2. Alpha is not supported in worker mode.

## Minimum supported Rust version (MSRV)

This crate requires Rust 1.89.0 or higher. Increases in MSRV will result in a semver
PATCH version increase.

## Changelog

### v0.6.0 — Precomputed reference, parallel pipeline, worker mode (2026-07-03)

- New `ReferenceFrame` type: the reference pyramid is computed once and reused for every
  score; a warm search iteration costs ~0.21 s instead of ~1.05 s on a 1448×1080 image.
- rayon parallelism extended to the whole pipeline (planes, independent blurs, maps,
  downscale, XYB conversion); the CLI is 2.4–2.7× faster across sizes.
- New `ssimulacra2_rs worker` subcommand: persistent process, raw pixels over stdin, no
  temp files or PNG round-trip per scored candidate.
- The binary now enables the lib's `rayon` feature (upstream disabled it via
  `default-features = false`).
- Fixed an lcms2 panic on 16-bit PNGs with an embedded ICC profile (transform `[u16; 3]`
  pixels instead of bare `u16`).
- Vertical blur pass parallelized by column stripes (identical values, ~15 % faster warm
  scores) and 16-bit RGB payloads (`fmt=2`) accepted in worker mode.
