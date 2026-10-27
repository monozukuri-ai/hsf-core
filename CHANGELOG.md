# Changelog

## Unreleased

Initial alpha API for bounded HSF syntax parsing and generic scene extraction:

- Header probing and selected 8.05, 14.50 and 19.10 syntax profiles, with retained
  source spans, internal zlib decoding and strict/partial parsing modes.
- Shell and rectangular Mesh geometry, selected primitives and text encodings,
  segment references, modelling transforms and definition ownership.
- Source-backed display attributes, bounded Style Segment resolution, line
  programs, basic glyph bindings, named views and camera metadata.
- Stored vertex parameter tuples of widths 1–128, selected quantized parameters
  and compressed normals, and bounded Null Shell refinement handling.
- Explicit experimental image and application User Data inventories.
- Shared byte budgets and limits on records, geometry, definitions and traversal.
- Public usage, support, scene, resource and integration guides.

See [Supported features and limitations](docs/supported-features.md) for the
scope of each feature and [Getting started](docs/getting-started.md) for API use.
