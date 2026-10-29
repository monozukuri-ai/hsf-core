# Changelog

## Unreleased (0.1.0-alpha.2, contract 34)

- `SceneOptions.mount_path`: resolve relative paths that climb above the file
  root through the producer's mount path; external results are explicit
  (`hsf.segment_path_external`, `hsf.reference_external`, `hsf.invalid_mount_path`).
- Repeat Object: `TK_Instance` operand order, the tristrip option bit, and
  in-place instances of tagged Null Shells (`NullShell.instance_of`,
  `instance_transform`; `hsf.repeat_source_unbound`, `hsf.repeat_target_bound`).
- `SceneOptions.placement = PlacementPolicy::BestEffort`: estimated instances
  under withheld state and after partial stops, labeled `hsf.placement_estimated`;
  rejected in strict mode (`hsf.placement_policy_rejected`).
- Include Library segments that are include targets render only through their
  includes, not at their own tree position.
- Capabilities add `mounted_root_relative_paths`,
  `repeat_object_null_shell_instances` and `best_effort_placement`.

## 0.1.0-alpha.1

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
