# Vendored packages

Third-party Rust crates packaged into this app (C# NuGet / DLL style).

| Crate | Version | Artifact |
|-------|---------|----------|
| secs4rs | 0.1.0 | `secs4rs-0.1.0.crate` |

- `secs4rs/` — extracted package used by Cargo (`path` dependency)
- `secs4rs-0.1.0.crate` — immutable package archive from `cargo package`

Upgrade:

```bash
# from simulator repo root
./scripts/vendor-secs4rs.sh /path/to/secs4rs
```
