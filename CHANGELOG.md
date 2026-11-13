# Changelog

## Unreleased

eDrawings drawings and third-party publishers read further: the CATIA V5 (HSF
19.00) and Creo 10 (21.00) publishers have syntax profiles, and several records
that drawings and older publishers write are framed.

Profiles

- `19.00` and `21.00` join the syntax profiles (`SYNTAX_VERSIONS`). 19.00
  follows the version-gated rules between 18.00 and 19.10, without the
  allowances observed only at 19.10; 21.00 is framed as 20.90 (precision byte,
  reference layout), with the Visibility bit 26 allowance.

Records

- Rendering Options read the transparency field that the transparency style
  (second-word bit 0x2, 7.10+) and transparency options (0x2000, 13.10+) select:
  one byte, a second when bit 7 is set, with the style in the low nibble, then
  the depth peeling layers (0x10) and minimum area (0x20). Z-sort preferences
  carry no operand; the peeling algorithm (0x40) and unknown bits stop with
  `hsf.transparency_options_mask`. The field is retained as
  `transparency_options`, after the hidden-line options.
- Clip regions take the 20.90 precision byte before their options (a nonzero
  value stops with `hsf.unsupported_clip_region_precision`), and the opaque
  option 5 is accepted at 18.00 and 20.90 as at 19.10. A 20.90 clip region was
  previously misread from the byte after its opcode.
- The empirical image profile accepts flags 0x87 (named JPEG) besides 0xa3
  (named local RGBA) and applies to 19.00 too. A JPEG-format image ends with its
  data: `ImageLayout.alpha_compression`/`alpha` and
  `ImageDefinition.alpha_compression`/`alpha_source` are now `Option` (`None`
  for these images), and its layout profile is `u32_jpeg_v1`.
- Rectangular Meshes with trivially quantized points (flag 1) are framed as
  Shell points are and decoded by the scene: the count must equal rows times
  columns. `GridMeshLayout.point_quantization` marks them.
- Compressed PolyPolylines (flag 1) are framed as trivial quantization: the
  scheme, the box of the unrestricted axes, the bits per coordinate (i32) and
  the packed codes; the shared or per-component values of one restricted axis
  follow as their own quantized block. The scene decodes both.
  `PolyPolylineLayout.point_quantization` and `leftover_quantization` mark
  them. Values of two or more axes, three 8-bit axes and quantized batches
  without an unrestricted axis stop (`hsf.poly_polyline_compression`,
  `hsf.poly_polyline_dimensions`). Uncompressed batches no longer need an
  unrestricted axis: every point can come from shared and per-component values.
- Shell attributes 5, 7 and 9 (colours on every vertex, for faces, edges and
  markers) are framed when trivially compressed with eight bits per channel and
  retained as unsupported attributes (`hsf.mesh_attributes`); other widths stop
  with `hsf.shell_attribute_compression`.
- TKE_Face_Pattern (`P`, one byte) is framed as `face_pattern` and treated by
  the scene as an unevaluated rendering setting; a named form (255) stops with
  `hsf.face_pattern`.
- TKE_Ellipse (`E`) and TKE_Elliptical_Arc (`e`) are framed: the center, a point
  on each axis and, for arcs, the parametric start and end. They become
  primitives of kinds `ellipse` and `elliptical_arc` with the three points;
  `PrimitiveLayout.limits` and `Primitive.limits` hold the arc's limits. Nothing
  is fitted or tessellated. Ellipses at 20.90 and later are unverified and stop
  (`hsf.unsupported_primitive_precision`).
- Colour records accept the ambient-down selector (0x2000000) at 19.10 on RGB
  colours as on colour names; other versions still stop with `hsf.color_mask`.
- Text encoding code 8 (UTF-16 in the published list) is decoded as
  little-endian UTF-16, like code 5.
- Capabilities add `transparency_options_framing`, `named_jpeg_images`,
  `quantized_mesh_points`, `quantized_poly_polylines`,
  `vertex_color_attribute_framing`, `face_pattern_framing` and
  `ellipse_primitives`.

## 0.1.0-alpha.6 (contract 38)

Connectivity-compressed Shells, as the Creo 11 eDrawings publisher writes them,
are decoded, and the scenes of large assemblies are built with much less memory.

Connectivity compression

- Shells with flag 0x40 (compression scheme 5, edgebreaker) are framed: the
  scheme byte, the stream's byte length and the stream, then the optional
  attributes. The stream header gives the point count.
  `ShellLayout.connectivity` (`ConnectivityLayout`) records the stream and its
  layout; `vertices` and `faces` are empty for such Shells.
- The scene decodes stream layout 2, as the Creo 11 eDrawings publisher writes
  it: C/L/E/R/S opcodes over a boundary loop (splits are located by a pre-pass
  over each component), dummy vertices removed with their triangles, and points
  and normals quantized within the stored box. Each component's first triangle
  is stored absolutely and every later vertex as a residual from parallelogram
  prediction. Decoded normals become the mesh's vertex normals unless the Shell
  stores its own.
- Merge opcodes (M, M2), patched vertices, boxes supplied by the toolkit and
  unknown table flags stop with `hsf.shell_connectivity_feature`; other schemes
  and layouts with `hsf.shell_connectivity_scheme`; streams without quantized
  points with `hsf.shell_connectivity_points`; malformed streams with
  `hsf.invalid_shell_connectivity`. `hsf.shell_connectivity_compression` is no
  longer reported.

Memory (breaking changes to public types)

- `SourceSpan.domain_id` and `SourceDomain.id` are `Arc<str>`, shared by every
  span of a domain.
- `Record`: the 23 optional layouts are one `layout: Option<RecordLayout>`,
  read with accessors of the old names (`shell()`, `user_data()`, ...); `id`
  and `scope` are `Arc<str>` and `kind` is `Cow<'static, str>`. Serialized
  records carry `layout` (an object tagged by layout name) instead of 23 keys.
- `DisplayAttributes`: optional attributes are `Option<Arc<Attribute<_>>>`, and
  `colors` and `visibility` are `Slots`, runs of slots sharing a value (read with
  `get(slot)`, `iter()` and `mask()`; still serialized as 32 entries).
- `Instance.path` and `PrimitiveInstance.path` are `Arc<[Arc<str>]>`, shared by
  the instances placed from one segment visit; `Instance::face_color()` and
  `face_visible()` read the display attributes instead of storing copies.
- Scene `source_record_id` fields are `Arc<str>`; scene `kind` fields,
  `OperandField` and `RenderingOperand` names and encodings, and
  `EntityMetadataField.kind` are `Cow<'static, str>`.
- Placed definitions share their local transform (none for the identity), and
  meshes, primitives, references and instances are sized from the syntax
  records within the configured limits. Budget charges computed from struct
  sizes follow the smaller structures; instance paths are charged where a new
  path is built.

## 0.1.0-alpha.5 (contract 37)

SOLIDWORKS 2011 and 2012 (HSF 17.85 and 18.00) and the Creo 11 eDrawings
publisher (21.15) are read, and two more records that SOLIDWORKS writes at 19.10
are framed. Creo 11 files read up to their connectivity-compressed Shells, which
remain unsupported.

- `17.85`, `18.00` and `21.15` join the syntax profiles (`SYNTAX_VERSIONS`). The
  existing version-gated rules apply to them; 21.15 is framed as 20.90,
  including the precision byte and the reference layout.
- TKE_Selectability (`!`) is framed as `selectability`: a geometry mask (a second
  byte when bit 7 is set), then the down, up, move-down, move-up and invisible
  masks, one byte each, or two when extended. It governs picking only, so the
  scene ignores it.
- Rendering Options accept simple reflection (second-word bit 0x80000, 15.50+),
  read after the simple shadow: an options byte (a second when bit 7 is set),
  then the plane, opacity, blur and attenuation its flags select. Unknown
  option bits stop with `hsf.simple_reflection_mask`. SOLIDWORKS writes it
  switched off, without further operands.
- Shells with quantized points (flag 1) are read when the scheme is 1 (trivial):
  the count, the box the codes span, 2–31 bits per coordinate, then the codes,
  packed as quantized parameters are. `ShellLayout.point_quantization`
  (`PointQuantization`: scheme, bits per sample, source span of the box) marks
  them, `vertices` then spans the codes, and the scene decodes them to f32
  vertices. Other schemes stop with `hsf.shell_point_compression`, which
  replaces `hsf.shell_quantized_points`, and other widths with
  `hsf.shell_point_bits`. An inverted box, or a code length that disagrees with
  the count and width, is invalid (`hsf.invalid_shell_point_bounds`,
  `hsf.invalid_shell_point_length`).
- The colour-lock selector 25/26 allowance now also covers 18.00, and the
  Visibility bit 26 allowance 21.15, where each was observed.

## 0.1.0-alpha.4 (contract 36)

SOLIDWORKS 2025 and 2026 write eDrawings as HSF 20.90, which alpha.3 refused at
the header. 20.90 differs from 19.10 as follows.

- `20.90` joins the syntax profiles (`SYNTAX_VERSIONS`).
- One byte follows the opcode of bounding (`B`, `b`), camera (`>`), view (`}`),
  modelling matrix (`%`), shell (`S`), the simple primitives (`X`, `l`, `L`, `G`,
  `C`, `c`, `T`, `x`), poly-polylines and images (`i`), before their
  single-precision operands. Only zero has been observed; any other value stops
  (`hsf.unsupported_bounds`, `hsf.unsupported_camera_projection`,
  `hsf.unsupported_view_projection`, `hsf.unsupported_matrix_precision`,
  `hsf.unsupported_shell_precision`, `hsf.unsupported_primitive_precision`,
  `hsf.image_profile`). Payloads exclude the byte, so the scene reads them as
  before.
- Include (`<`) and style (`{`) references start with a zero byte and carry no
  condition after the path, whereas a leading zero before 20.90 announces one.
  A nonzero first byte stops with `hsf.unsupported_conditional_segment`.
- Shell attribute 28 has the layout of 27: a width, then a float per vertex and
  component (widths 2 and 5 observed). It is framed as its own family; the scene
  still decodes the mesh and reports the attribute with `hsf.mesh_attributes`.
- The 19.10 allowances for Visibility bit 26 and colour-lock selectors 25/26 and
  the empirical image profile also apply to 20.90, where they were observed.
- `hsf.shell_attribute` names the unsupported attribute code in its message.

## 0.1.0-alpha.3 (contract 35)

eDrawings 2026 (HOOPS 3DF 20.10 writer behind a 19.10 header) stopped alpha.2
at four places, which are now framed.

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
  (one `configN` root child per configuration) can be placed one
  configuration at a time. `user_option_text()` decodes option records from the
  syntax so callers can pick the subtree before the scene exists.
- Memory: `Record` keeps its optional layouts boxed (1,920 → about 400 bytes
  per record) and `Instance`/`PrimitiveInstance.display` is an
  `Arc<DisplayAttributes>` shared by the instances of one segment visit. The
  traversal budget charges the real sizes (path bytes, instance struct,
  inherited attributes per visited segment) instead of a flat 16 KiB per visit.
  JSON output is unchanged; Rust callers deref the boxes and the Arc.
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
