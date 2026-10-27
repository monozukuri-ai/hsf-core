# Adapter integration and experimental profiles

Adapters own outer containers, application semantics and any explicitly selected
empirical framing. The generic crate does not supply product-specific revision
rules, length accounting, document mapping or external resource resolution.

## Precharged input

`parse_precharged(Arc<[u8]>, options, budget)` retains owned input without making
the standalone parser's copy. The caller must first charge input ownership and
any outer decompression to the same `Budget`. The entry point neither resets nor
recharges those counters.

This minimal example accounts for an already owned, uncompressed HSF stream:

```rust
use hsf_core::{Budget, Limits, ParseOptions, Status};
use std::sync::Arc;

fn main() -> Result<(), hsf_core::HsfError> {
    let bytes = b";; HSF V19.10 \n\0";
    let mut budget = Budget::new(Limits::default());
    budget.charge_input(bytes.len() as u64)?;
    budget.charge_output(bytes.len() as u64)?;
    let owned: Arc<[u8]> = Arc::from(bytes.as_slice());
    let parsed = hsf_core::parse_precharged(
        owned, &ParseOptions::default(), &mut budget,
    )?;
    assert_eq!(parsed.status, Status::Complete);
    Ok(())
}
```

For a container adapter, account for outer input, expanded bytes and retained
buffers as they are read/allocated. Do not charge the same ownership twice.
Additional adapter copies and serialization need their own charges/policy.
See [Resource limits](resource-limits.md).

## Build identity

`build_info()` returns package version, contract version, source SHA-256 and
capabilities. `VERSION`, `CONTRACT_VERSION` and `SOURCE_SHA256` are also exported.
The fingerprint covers Cargo.toml, build.rs and src/**/*.rs, using sorted relative
paths and NUL-separated names and bytes. It does not cover docs or test data.
A manifest change can change the fingerprint without changing Rust behavior.
An adapter can record this identity alongside its own format/profile version;
a Cargo lockfile alone does not pin uncommitted path-dependency source.

## Application User Data

User Data (`[`) is unsupported by default. A trusted adapter can opt into
`parse_precharged_with_user_data(data, options, budget, Some(&profile))` by
implementing `UserDataProfile`. The profile receives the header, domain/source
context, segment depth, prior records and a borrowed framing window beginning
at the opcode. `max_framing_bytes()` defaults to 4096 bytes; requests are clamped
to a hard 1 MiB cap. Domain length and optional parent suffix provide context,
not permission to seek or resynchronize.

The backend independently validates the returned declared word, stop marker and
field ranges. Metadata is limited to 64 fields, 64-byte labels and a 128-byte
profile identifier; 16384 output bytes are charged before the callback. Field
offsets remain absolute within their domain. Successful framing retains
`Record.user_data`, emits `hsf.experimental_user_data` and leaves the result
partial even if no bytes remain unparsed. Strict mode rejects every empirical
profile. Unknown profiles must not trigger marker searches.

Scene processing normally stops with `hsf.user_data_semantics`. An adapter may
explicitly name one root record before child nodes or geometry using
`SceneOptions.inventory_user_data_record`. This continues local inventory,
retains `opaque_user_data`, withholds all placements and leaves placement status
unsupported. A separate `inventory_terminal_user_data_record` requires that
first authorization, root scope, final User Data / Stop Compression / termination
records and complete contiguous domain/parent termination. It never enables
placements. Defaults leave both authorizations absent.

## Experimental images

`ParseOptions.image_profile = ImageProfile::U32RgbaJpegV1` enables a bounded
empirical HSF 19.10 profile: finite XYZ, u32 flags 0xa3, a nonempty byte-length
name, positive i32 dimensions, JPEG data and constant/RLE alpha. The default is
`ImageProfile::Disabled`; strict mode rejects the empirical profile. Partial
mode emits `hsf.experimental_image` and never claims complete syntax.

The JPEG check validates a baseline single-scan envelope and matching dimensions;
it does not decode entropy data, quantization/Huffman tables or pixels. RLE alpha
is counted without expansion and must fill exactly width × height pixels.
`max_image_pixels` bounds the cumulative dimensions. Other flags/versions and
JPEG profiles stop at the opcode; malformed recognized operands error.

Scene inventory retains encoded image definitions, source spans, names, owners,
dimensions and reference points. Images become taggable objects so later tags
cannot attach to preceding geometry. Inventory remains partial, emits
`hsf.image_inventory` and withholds all placements. Image placement,
Repeat/Reopen handling, pixel/color conversion and external image loading remain
unsupported. Bounded texture/image association is described in
[Scene data](scene-data.md#views-rendering-events-and-textures).

## Import context and aliases

`SceneOptions.import_root` optionally supplies an absolute unquoted ASCII path
of at most 4096 bytes. Context ancestors have no file sources; relative file
paths start at the import node while absolute paths and standard aliases keep
their database-root meaning. `Scene.import_context` records the requested and
normalized paths, import node and context nodes.

This is caller-supplied context, not a recovered producer location or external
database. It is partial-only, emits `hsf.import_context` and withholds all
placements. Invalid path grammar errors. Context nodes consume node/depth/output
budgets, and external targets are not loaded.

Partial inventory can assume startup bindings for `?Home`, `?Include Library`
and `?Style Library`. `segment_aliases` records the binding and first-use source.
Unknown runtime overrides leave warnings; strict mode rejects the assumptions
and all placements remain withheld. Unrecognized aliases are unsupported.

[Supported features](supported-features.md) · [Documentation index](README.md)
