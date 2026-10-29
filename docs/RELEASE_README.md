# malware-organizer

`malware-organizer` ingests ZIP archives into content-addressed storage without executing or opening extracted payloads. It records provenance and builds metadata-driven views with runtime TOML rules.

The default `sealed` mode stores payloads with owner-only access removed (`0000`) after streaming validation. This is a permission policy, not a sandbox; use a dedicated unprivileged account and an isolated VM or container for malware handling.

Typical commands:

```text
malware-organizer inspect ./downloads
malware-organizer ingest ./downloads --output ./repository --rules ./rules.toml
malware-organizer stats ./repository
malware-organizer reorganize ./repository --rules ./rules.toml
```

See the repository README and `config.example.toml` for limits, passwords, security modes, and platform support. The release currently supports Unix permission-backed storage; unsupported storage backends fail closed.
