# Reliability, security and performance

[Documentation hub](README.md) · [Command reference](commands.md)

## Supported platform

bfsort supports Linux. Repository storage requires Unix permissions; unsupported
storage platforms fail explicitly. No Windows or macOS executables are distributed.

## File handling

bfsort streams inputs into private temporary storage while computing SHA-256 and
checking configured size and expansion limits. Archive paths, entry kinds,
passwords and integrity checks are validated before publication. Stored objects
are deduplicated by hash; occurrence metadata preserves distinct source names
and provenance.

Sealed payloads have no permission bits (`0000`). Standard payloads use `0600`,
and managed directories use `0700`. bfsort never automatically unlocks an existing
payload. Permissions are not a sandbox: the owner or root can change them.
Use an isolated environment when handling untrusted content.

ZIP exports and report files refuse to overwrite existing destinations. View
reorganization retains previous generations. Interrupted ingestion can leave
private staging data, which subsequent ingestion recovers under the repository
lock. Existing source files remain untouched.

## Detection and reports

Content detection uses bounded signatures, header checks and ZIP directory
markers. It does not execute files, render documents or establish whether a file
is safe. A mismatch means that a recognized filename extension contradicts
strong content evidence.

Unknown, ambiguous and unevaluated records remain explicit. Metadata-only reports
work on sealed repositories. Payload exports and explicit analysis require a
standard repository. Export trusts stored identities and classification metadata;
it does not rehash objects or perform an integrity audit.

See [content detection](detection.md) for supported formats, compatibility
mappings, inspection limits and report examples. CSV preserves raw source values;
import untrusted filename columns as text in spreadsheet software.

## Performance

Worker count, file sizes, compression, duplicate frequency and filesystem latency
all affect throughput. More workers do not always improve performance. The
detector reuses the 8 KiB prefix captured during hashing; only ZIP candidates
require additional bounded metadata reads. Repository-wide reports retain
metadata in memory, so memory use grows with collection size.

The benchmark harness uses synthetic benign inputs. Recorded timings are examples,
not throughput guarantees for another machine or dataset:

- [Ingestion before detailed detection](benchmark-detection-before.jsonl)
- [Ingestion with detailed detection](benchmark-detection-after.jsonl)
- [Detector microbenchmark](benchmark-detection-micro.jsonl)

The ingestion comparison covers five workloads, five worker counts and both
security modes. Quick-mode workloads use 100 small or duplicate archives, one
archive with 100 entries, or four 1 MiB archives. Results vary in both directions;
the small in-memory detector benchmark does not predict end-to-end throughput.

## Developer checks

To verify a local build:

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked
cargo nextest run --locked
```

Some integration tests start local HTTP servers. `nextest` is optional.
To measure synthetic performance:

```sh
cargo bench --locked --bench detection
BFS_BENCH_QUICK=1 cargo bench --locked --bench throughput
```

Omit `BFS_BENCH_QUICK` for larger workloads. The fuzz harness requires nightly
Rust and cargo-fuzz:

```sh
cargo +nightly fuzz run untrusted_inputs -- -max_total_time=30 -max_len=65536
```

Tests and short fuzz runs cannot establish complete parser correctness, universal
classification accuracy or resilience to every filesystem failure.
