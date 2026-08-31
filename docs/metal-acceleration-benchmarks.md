# Metal Acceleration Benchmarks

These are representative local measurements for the hybrid GPU path. They are
not release-performance guarantees; rerun them on each supported Apple Silicon
class before changing dispatch thresholds.

## Environment

- Host: Apple M4, arm64
- macOS: 27.0 (build 26A5416b)
- Memory: 16 GB
- Build: `cargo build --release --features gpu`
- Database: a clean checkout of GraphIQ itself, 185 files, 2,109 symbols, 6,564 edges
- Benchmark: 5 warmups and 50 uncached iterations per query

## Results

The exhaustive benchmark scores all 2,109 symbols. It is intentionally a large
numeric scoring batch so the Metal kernel can be measured without the normal
FTS seed cap:

| Mode | Median | P95 | GPU dispatches |
|---|---:|---:|---:|
| Metal | 16,058 us | 22,781 us | 165 |
| CPU (`GRAPHIQ_DISABLE_GPU=1`) | 21,350 us | 22,171 us | 0 |

Metal was approximately 25% faster at median latency for this workload. Its
P95 was slightly higher because synchronous buffer readback is sensitive to
host/GPU scheduling.

Command:

```bash
cargo build --release --features gpu
./target/release/graphiq index . --db /tmp/graphiq-metal.db
GRAPHIQ_GPU_TRACE=1 ./target/release/graphiq-bench metal-exhaustive /tmp/graphiq-metal.db 50
GRAPHIQ_DISABLE_GPU=1 ./target/release/graphiq-bench metal-exhaustive /tmp/graphiq-metal.db 50
```

The regular seeded benchmark did not dispatch Metal for the current GraphIQ
index because its candidate batches remain below the default threshold of 512
candidates and 32,768 estimated work items. This is intentional: forcing Metal
for those smaller interactive searches increased latency on the same machine.

A candidate-limit sweep on the same clean checkout confirmed the dispatch gate:

| Exhaustive candidate limit | GPU dispatches (10 iterations × 3 queries) |
|---:|---:|
| 1 | 0 |
| 10 | 0 |
| 50 | 0 |
| 200 | 0 |
| 2,109 | 45 |

The `metal-exhaustive` command accepts arbitrary limits, so larger 10k/100k
fixtures can be added without changing the benchmark code.

## Interpretation

- Keep CPU as the default for small interactive searches.
- Retain the automatic GPU path for genuinely large scoring batches.
- Re-measure thresholds on M-series generations and before adding more GPU
  stages.
- The macOS smoke workflow lowers the thresholds only for a small deterministic
  fixture so CI proves Metal execution and CPU result parity; those overrides
  are not production defaults.
