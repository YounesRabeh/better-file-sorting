# Implementation and validation record

[< Back to malware-organizer](../README.md) • [Documentation Hub](README.md)

Validated on 2026-10-29 using Rust 1.99.0, Linux x86-64, as uid 1000. This is a pre-release implementation, not a v1.0 release certification.

## Plan coverage

| Stage | Delivered | Remaining release evidence |
|---|---|---|
| 0 — Foundation | Rust 2024 package, clap commands, strict TOML/CLI precedence, errors, logging, progress, examples, CI | Hosted CI has not been dispatched |
| 1 — Inspection | Content discovery, bounded-input ZIP parser, metadata-only inspection, encryption flags, compression support reporting, symlink/output exclusions | Exotic ZIP extensions intentionally unsupported |
| 2 — Extraction | Streaming, SHA-256, bounded prefix, per-entry password retries, AES/ZipCrypto, actual byte/ratio/count limits, integrity checks and cleanup | Long-running adversarial fuzz campaigns remain future release work |
| 3 — Storage | Content classification, no-overwrite content-addressed objects, durable provenance, repeat-ingest semantics | Content magic remains heuristic |
| 4 — Pipeline | Bounded Rayon archive workers, serialized durable journal, failure isolation, Ctrl-C, forced-stop recovery | Total collection metadata scales with input; no constant-RAM claim |
| 5 — Security | Unix standard/sealed permissions, prepublication sealing, protected staging, no automatic unsealing, fail-closed unsupported platforms | Windows ACL storage backend; runtime macOS/Windows filesystem testing |
| 6 — Rules | Runtime first-match rules, all duplicate policies, sanitized templates, deterministic collisions, hardlink/manifest views, exact reorganization preview, versioned replacement | Hostile same-account concurrent path replacement is outside the permission boundary |
| 7 — Reporting | JSONL provenance/failures, run IDs, JSON summaries, category/duplicate/orphan/physical storage statistics, throughput and exit codes | None for the supported Unix path |
| 8 — Hardening | Synthetic functional/security tests, sanitizer fuzz smoke, dependency audit, release builds, cross-target compile checks, benchmark matrix | Power-loss fault injection, prolonged fuzzing and hosted platform runs before v1.0 |

## Checks

- Formatting, Clippy with warnings denied, nextest, documentation tests and optimized build pass on Linux.
- 39 integration tests pass, including actual ordinary-user read/write/direct-execution denial, AES and ZipCrypto fixtures, corruption, archive bombs, malicious paths, concurrency, duplicate provenance, non-UTF-8 paths, rules, metadata-only reorganization and SIGKILL/recovery.
- Windows GNU and macOS Darwin targets pass compilation and Clippy with warnings denied; their runtime permission behavior has not been tested here.
- `cargo audit` found no vulnerabilities in the 151 root-package dependencies against the database fetched during this session.
- Fuzzing passed 317,974 executions in 31 seconds with AddressSanitizer. A preceding 477,121-execution run finished its fuzz loop but failed at LeakSanitizer shutdown because the sandbox blocks process inspection. The successful rerun used `ASAN_OPTIONS=detect_leaks=0`, preserving address checks; leak detection is **not verified**. Host ccache was bypassed with `CCACHE_DISABLE=1 CXX=/usr/bin/clang++` because its cache is read-only here.
- Cross-platform CI, audit and fuzz-smoke workflows are provided. No push was made to trigger them.

## Decisions made during execution

1. Reused the current checkout on `codex/malware-organizer`; no commit or push. All work remains reviewable locally.
2. Enabled Stored/Deflate and AES/ZipCrypto only; other decoders fail explicitly. This reduces the parser surface but excludes archives using other compression methods.
3. Windows storage fails closed until an ACL backend exists. It does not silently weaken either security mode; Windows users can currently inspect archives only.
4. Repeated identical source path/index/content is one logical occurrence, while all ingestion events are retained. This gives idempotent logical views; a consumer wanting run-event counts must read raw manifests.
5. Staging files start with `0000` permissions and use their creation descriptor for processing. This resolves the plan's conflict between delaying restrictive permissions and protecting forced-stop leftovers. Final object policy is still applied after verification. It relies on POSIX descriptor semantics and is tested on Linux.
6. Previous complete view generations are retained; only abandoned staging is cleaned automatically. This preserves recovery options at the cost of retained directory entries/hard links.
7. A dependency-free end-to-end benchmark with assertions replaces Criterion. Large durable-ingestion measurements are single runs, so they are noisy and not statistical guarantees.
8. Ingest dry-run validates rules and previews archive metadata; exact hash/type/duplicate layout is previewed by reorganize after ingestion. No exact content-derived preview is claimed without reading payloads.

## Independent review

A fresh read-only reviewer identified silent archive-cap omissions, lossy path identities, reserved/file-directory view collisions, Java/Mach-O magic ambiguity, incomplete directory synchronization and omitted orphan storage accounting. These were fixed; behavioral defects have regression tests observed failing before their fixes. Directory synchronization was added for manifests, object shards and nested views, but this is not a substitute for power-loss testing. No minor review findings were deferred.

## Benchmarks

The full matrix covers 50 combinations: five workloads × five worker counts × two security modes. All payloads are synthetic and harmless. Small/duplicate workloads use 10,000 archives; large compressible/incompressible workloads use 100 archives of 1 MiB each; many-entry workloads use one ZIP with 10,000 entries. Reorganization uses 100 rules and all occurrences. These are baseline workload sizes, not a claim that 1 MiB represents every real-world large archive.

Results are single runs on a shared host, with warm caches and other build/test work possible. Do not infer universal worker scaling. Resource totals include dataset generation and reorganization, while each JSONL ingestion timing excludes dataset generation. Allocated bytes measure canonical payload blocks, excluding metadata and directories.

Full tmpfs matrix: **50/50 cases passed**, including per-run accepted-entry/failure assertions. The additional Btrfs/NVMe pass was stopped after eight completed measurements because timings became highly variable and durable-write repetitions were lengthy. It is a partial disk baseline, not a completed disk matrix.

Raw results: [tmpfs](benchmark-tmpfs.jsonl), [partial Btrfs](benchmark-btrfs-partial.jsonl).

| Workload, sealed mode | 1 worker files/s | 4 workers files/s | 16 workers files/s |
|---|---:|---:|---:|
| small | 7713.5 | 11508.7 | 11926.1 |
| large-compressible | 852.6 | 1463.6 | 1824.4 |
| large-incompressible | 786.4 | 2022.5 | 1828.0 |
| many-entries | 11120.9 | 11327.3 | 11745.5 |
| duplicates | 20071.1 | 36562.5 | 33777.5 |

Resource meter for the complete tmpfs harness (includes fixture creation):

```text
Command being timed: "target/release/deps/throughput-c98f2e1688f04de4"
	User time (seconds): 18.14
	System time (seconds): 30.54
	Percent of CPU this job got: 120%
	Elapsed (wall clock) time (h:mm:ss or m:ss): 0:40.27
	Average shared text size (kbytes): 0
	Average unshared data size (kbytes): 0
	Average stack size (kbytes): 0
	Average total size (kbytes): 0
	Maximum resident set size (kbytes): 34748
	Average resident set size (kbytes): 0
	Major (requiring I/O) page faults: 0
	Minor (reclaiming a frame) page faults: 74859
	Voluntary context switches: 4116
	Involuntary context switches: 27063
	Swaps: 0
	File system inputs: 0
	File system outputs: 48
	Socket messages sent: 0
	Socket messages received: 0
	Signals delivered: 0
	Page size (bytes): 4096
	Exit status: 0
```
