# bfsort

[< Back to bfs](../README.md) • [Documentation Hub](README.md)

`bfsort` ingests ZIP archives into content-addressed storage without executing or opening extracted payloads. It records provenance and builds metadata-driven views with runtime TOML rules.

The default `sealed` mode stores payloads with owner-only access removed (`0000`) after streaming validation. This is a permission policy, not a sandbox; use a dedicated unprivileged account and an isolated VM or container for malware handling.

Common workflows:

```sh
bfsort organize ./incoming --output ./organized
bfsort ingest ./incoming --extract --output ./organized
bfsort stats ./organized
bfsort report mismatches ./organized --format csv --output mismatches.csv
bfsort reorganize ./organized --rules ./examples/rules.example.toml
```

Use `--extract` explicitly to ingest ZIP contents. Ordinary files are sorted by
default. Content detection and mismatch reports use stored metadata; a mismatch
is not a malware verdict.

For independent ZIP exports, ingest with `--security-mode standard`, then use
`bfsort export` with a sorting policy. Sealed repositories support metadata reports
and export previews without unlocking payloads.

See the [command reference](commands.md), [configuration guide](configuration.md)
and [detection and export guide](detection.md) for options and examples.
Releases support Linux only.
