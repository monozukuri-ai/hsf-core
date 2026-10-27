# hsf-core

An independent Rust reader for HSF (HOOPS Stream Format), with bounded syntax
parsing and generic scene extraction. It preserves source locations, reports
unsupported data explicitly, and runs without a vendor SDK or runtime.

The crate supports selected **8.05, 14.50 and 19.10** profiles. It is an alpha
library, not a complete HSF implementation, renderer or CAD document reader.
Requires **Rust 1.88 or later**.

## Getting started

After the initial alpha is published to crates.io, add:

```toml
[dependencies]
hsf-core = "0.1.0-alpha.1"
```

For a local checkout, use `hsf-core = { path = "../hsf-core" }` instead.

Parse a minimal HSF stream and build a scene using a shared resource budget:

```rust
use hsf_core::{Budget, Limits, ParseOptions, SceneOptions, Status};

fn main() -> Result<(), hsf_core::HsfError> {
    // Header followed by a termination opcode: a valid empty scene.
    let bytes = b";; HSF V19.10 \n\0";
    let mut budget = Budget::new(Limits::default());
    let parsed = hsf_core::parse(bytes, &ParseOptions::default(), &mut budget)?;
    let scene = hsf_core::scene(&parsed, &SceneOptions::default(), &mut budget)?;

    assert_eq!(parsed.status, Status::Complete);
    assert_eq!(scene.status, Status::Complete);
    assert!(scene.data.meshes.is_empty());
    Ok(())
}
```

Both stages default to partial mode. For arbitrary input, inspect status,
diagnostics and scene placement status before using the result. Strict mode
rejects unsupported input; malformed data and exhausted limits error in both
modes. See [Getting started](docs/getting-started.md) for file input and strict
parsing.

## What it provides

- Syntax records, retained source bytes and checked spans, including internal
  zlib domains.
- Selected Shell and rectangular Mesh geometry, drawing primitives, text,
  segment references and transforms.
- Source-backed display attributes, views, stored vertex parameters and bounded
  style/resource inventories.
- Shared byte budgets, object/traversal limits and explicit unsupported results.

Support depends on each record's version, flags and operands. Unsupported framing
stops at the original opcode without scanning ahead. Decoded metadata does not
imply evaluated rendering, complete placements or native application compatibility.
Image and application-specific User Data profiles require explicit opt-in and
remain partial. See the [support guide](docs/supported-features.md).

## Documentation

- [Documentation index](docs/README.md)
- [Getting started and error handling](docs/getting-started.md)
- [Supported features and limitations](docs/supported-features.md)
- [Scene data and interpretation](docs/scene-data.md)
- [Resource limits and source provenance](docs/resource-limits.md)
- [Adapter integration and experimental profiles](docs/integrations.md)
- [Changelog](CHANGELOG.md)

## License

Licensed under [Apache-2.0](LICENSE); see [NOTICE](NOTICE). This is an independent
implementation informed by the public
[HSF specification](https://docs.techsoft3d.com/hoops/visualize-3df/general/hsf/HSF_architecture.html),
not an official HOOPS SDK component.
