# Changelog

## Unreleased (0.1.0-alpha.3, contract 35)

eDrawings 2026 (HOOPS 3DF 20.10 writer behind a 19.10 header) stopped alpha.2
at four places; each is now framed from the shipped toolkit's grammar.

- TKE_Priority (`0`, one i32 operand) is framed as `priority` and ignored by the
  scene. eDrawings 2026 writes it before component segments; alpha.2 stopped at
  the opcode and left the rest of such files unparsed.
- TKE_User_Index_Data (`m`) is framed as `user_index_data`: a count, then per
  entry a signed index, a size and that many opaque bytes (interleaved, unlike
  User Index). The scene lists them per segment in `user_index_data` with status
  `unsupported`; eDrawings 2026 stores display names under the index spelled
  `name`. `hsf.limit_user_index_data` bounds the count.
- Rendering Options read the third mask/value word that follows when bit 31 of
  the second word (`TKO_Rendo_Extended2`) is set. `RenderingLayout` gains
  `extended2_mask`/`extended2_value` (zero when absent). Only switches without
  operands are accepted in the third word (frame buffer effects, scaled
  displacement, bump-mapping parallax, randomize vertices) and in the second
  (index markers, Gooch lighting, transparency depth writing); options whose
  operand position is unverified still stop with `hsf.rendering_options_mask`.
- Segment paths accept backquoted components (`` `eDrawings` ``) as literal,
  case-insensitive names; quotes are not part of the name, navigation and alias
  meanings do not apply inside them, and a literal that would read as navigation
  is stored re-quoted. Slashes inside quotes, text glued to a quote, nested or
  unbalanced quotes stay unsupported.
- `SceneOptions.traversal`: `All` (default), `Subtree(path)` places only one
  absolute literal subtree plus its includes (`hsf.traversal_subtree` info;
  unknown paths place nothing with `hsf.traversal_root_unknown`), `Skip` builds
  the inventory without placements. Multi-configuration eDrawings assemblies
  (eleven `configN` root children, 1.6 million instances) are placed one
  configuration at a time. `user_option_text()` decodes option records from the
  syntax so callers can pick the subtree before the scene exists.
- Memory: `Record` keeps its optional layouts boxed (1,920 → about 400 bytes
  per record; a 745k-record assembly parses in 0.4 GB instead of 1.5 GB) and
  `Instance`/`PrimitiveInstance.display` is an `Arc<DisplayAttributes>` shared
  by the instances of one segment visit. The traversal budget charges the real
  sizes (path bytes, instance struct, inherited attributes per visited segment)
  instead of a flat 16 KiB per visit. JSON output is unchanged; Rust callers
  deref the boxes and the Arc.
- Capabilities add `segment_priority_framing`, `user_index_data_framing`,
  `rendering_options_extended2_framing`, `backquoted_segment_names` and
  `traversal_subtree`.

## 0.1.0-alpha.2 (contract 34)

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
