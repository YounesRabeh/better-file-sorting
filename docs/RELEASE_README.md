# bfs

[< Back to bfs](../README.md) • [Documentation Hub](README.md)

`bfs` ingests ZIP archives into content-addressed storage without executing or opening extracted payloads. It records provenance and builds metadata-driven views with runtime TOML rules.

The default `sealed` mode stores payloads with owner-only access removed (`0000`) after streaming validation. This is a permission policy, not a sandbox; use a dedicated unprivileged account and an isolated VM or container for malware handling.

Typical commands:

```text
bfs inspect ./downloads
bfs ingest ./downloads --output ./repository --rules ./rules.toml
bfs stats ./repository
bfs reorganize ./repository --rules ./rules.toml
```

See the repository README and `config.example.toml` for limits, passwords, and security modes. Releases and CI support Linux only; no Windows or macOS executables are distributed.
