# Getting started

## Dependency and API

The crate requires Rust 1.88 or later. Once the initial alpha is published to
crates.io, add:

```toml
[dependencies]
hsf-core = "0.1.0-alpha.3"
```

To use a local source checkout before publication or during development, replace
the dependency with `hsf-core = { path = "../hsf-core" }`, adjusting the path to
your checkout.

| API | Purpose |
| --- | --- |
| `probe`, `read_header` | Recognize a header using at most 64 bytes |
| `parse` | Copy and parse input with resource accounting |
| `scene` | Extract a bounded generic scene from a parse result |
| `capabilities` | List implemented version profiles, opcodes and scene features |
| `build_info` | Identify the package, API contract and source fingerprint |
| `parse_precharged` | Parse owned input already accounted for by an adapter |

Header recognition does not validate the body or establish version support.
`Version.major` and `Version.minor` are integers; `Version.text` preserves the
version spelling. `capabilities()` describes available profiles, not whether a
particular file can be fully interpreted.

## Reading a file in strict mode

This complete program accepts a file path. It checks the file size before reading
through a capped reader, then uses the same budget for parsing and scene creation.

```rust
use hsf_core::{Budget, Limits, ParseMode, ParseOptions, SceneOptions};
use std::{fs::File, io::{self, Read}};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "usage: reader <file.hsf>")
    })?;
    let limits = Limits::default();
    let input_limit = limits.max_input_bytes;
    let file = File::open(path)?;
    if file.metadata()?.len() > input_limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "input too large").into());
    }
    let mut bytes = Vec::new();
    file.take(input_limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > input_limit {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "input too large").into());
    }

    let mut budget = Budget::new(limits);
    let parsed = hsf_core::parse(
        &bytes,
        &ParseOptions { mode: ParseMode::Strict, ..Default::default() },
        &mut budget,
    )?;
    let scene = hsf_core::scene(
        &parsed,
        &SceneOptions { mode: ParseMode::Strict, ..Default::default() },
        &mut budget,
    )?;
    println!("{} meshes, {} primitives",
        scene.data.meshes.len(), scene.data.primitives.len());
    Ok(())
}
```

`parse` retains its own input copy; the caller's `bytes` buffer is separate. For
already owned/container-extracted data, see [Adapter integration](integrations.md).

## Partial results and errors

`ParseOptions` and `SceneOptions` default independently to `ParseMode::Partial`.
Set both to `Strict` when the whole operation must reject unsupported syntax and
scene semantics.

- **Partial:** unsupported framing stops at that opcode. The result retains the
  decoded prefix, diagnostics and unparsed source spans. Scene processing may
  retain local definitions while withholding placements or omitting an
  unsupported reference branch.
- **Strict:** unsupported syntax or scene semantics return `HsfError`. A strict
  scene also rejects an incomplete syntax result.
- **Both modes:** malformed/truncated known operands, invalid numeric data, bad
  zlib checksums and exhausted limits return errors. Partial mode is not a way
  to accept corruption.

Each `ParseOutcome<T>` exposes `data`, `status`, `diagnostics`, `unparsed` and
cumulative `usage`. Inspect syntax and scene outcomes separately. Scene
`processed_records` reports semantic coverage; it is independent of byte framing.
Empty `unparsed` spans alone do not imply complete interpretation: experimental
framing can consume every byte and still return a partial result.

Before using instances, inspect `Scene.placement_status` and diagnostics. Empty
instance arrays may mean placements were withheld, rather than an empty file.
`SceneOptions.placement = PlacementPolicy::BestEffort` returns estimated instances
instead, labeled by `hsf.placement_estimated`; see [Scene data](scene-data.md#placement-policy).
`HsfError.diagnostic` includes a code, classification, stage and, when available,
source/opcode/version context. Other result objects expose their own decoding,
association or interpretation status; a complete association does not establish
complete rendering semantics.

[Supported features](supported-features.md) · [Resource limits](resource-limits.md)
