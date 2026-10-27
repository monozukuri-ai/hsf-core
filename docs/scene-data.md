# Scene data and interpretation

`scene(&parsed, &options, &mut budget)` returns `ParseOutcome<Scene>`. Keep the
syntax result available to resolve the scene's source spans. Syntax coverage,
scene coverage and placement validity are separate; see
[Getting started](getting-started.md#partial-results-and-errors).

## Definitions, ownership and instances

`Scene.meshes` and `Scene.primitives` contain local geometry definitions.
`Scene.instances` and `Scene.primitive_instances` contain evaluated placements.
Definitions can remain available when placements are withheld. Inspect
`placement_status`, `processed_records` and diagnostics before consuming them.

`Mesh.owner_node_id` and `Primitive.owner_node_id` identify the defining segment.
Includes, references and repeats preserve that owner. A supported Null Shell
refinement keeps its target's owner; `refinement.context_node_id` identifies
where the update arrived. Ownership does not identify an application sheet,
component or world-space placement.

Mesh vectors retain vertices, face values, triangles, source-face indices,
normals and stored parameters. Rectangular Mesh faces are derived: each cell
uses the `(r+1,c)`–`(r,c+1)` diagonal and sequential triangle-face IDs. Shell
triangulation can be unsupported while the original arrays remain available.
`Primitive.part_lengths` separates PolyPolyline components; no connecting lines
are added between components. The whole batch remains one reference/tag target.
See [Supported features](supported-features.md#geometry-and-text).

`null_shells` preserves placeholder/tag identity. A supported unique LOD 0 update
links `NullShell.mesh_id` to a source-backed mesh. Duplicate updates revoke this
link rather than selecting a winner. `shell_parameter_sets` preserves parameter
inventories and separate association/interpretation statuses for other
refinement and LOD records; association alone does not replace geometry.

## Coordinates, matrices and handedness

All geometry remains in local source coordinates, with no inferred units.
Modelling matrices expand 12 stored f32 values into row-major f64 4×4 matrices.
Composition uses row vectors and `local * parent`. Instance matrices expose the
placement; the crate does not bake them into vertices or normals. Determinants
expose reflections, and triangle winding is not flipped automatically.

World handedness inherits with provenance. Missing handedness remains absent;
neither handedness nor view metadata changes stored coordinates. Texture
matrices are separate: a local Set replaces earlier values and inherited values
without concatenation, and never changes geometry coordinates.

## Segments, paths and styles

Unquoted ASCII literal paths use case-insensitive names, collapsed/trimmed spaces
and `.`/`..`/`^` navigation. Source spelling is retained. Open Segment can create
missing literal parents with `implicit = true` and the creating record's source;
Close returns to the original scope. Anonymous segments are supported, but
relative paths from them are not. Quotes, wildcards, path lists, unknown aliases,
non-ASCII names and root-escaping paths remain unsupported.

References resolve previously known literal paths/tags. Reopen uses final segment
attributes. Repeat Object supports previously tagged geometry, variants 0/new
target (-1), and options 0. Unresolved references, cycles and conditional includes
omit their branch with diagnostics; unsupported scope can withhold all placements.
Traversal depth/work remains bounded even when expansion yields no geometry.

`style_segments` retains ordered edges and source expressions. The supported
unconditional subset resolves display attributes without including style-target
geometry or inheriting the target's parent attributes. Local values override
styles; styles override the actual instance-path parent's values.
`style_resolutions` exposes combined values and unsupported reasons, while
`Node.display` retains local attributes. Conditions, cycles, overlapping
providers, missing targets, unknown effects and incomplete prefixes fail closed.

Startup aliases (`?Home`, `?Include Library`, `?Style Library`) and caller-supplied
import roots are explicit partial-inventory mechanisms, described in
[Adapter integration](integrations.md#import-context-and-aliases).

## Display attributes, line programs and glyphs

`DisplayAttributes` retains sources and inheritance for RGB/visibility, marker
size, line/edge weights, patterns, marker symbols, text alignment and texture
matrices. Color/visibility arrays have 32 slots; extension flags are not geometry
attributes. Sizes retain their magnitude and relative/object/screen/window/
points/pixels units without conversion. Missing attributes remain absent.
HSF 19.10 Visibility bit 26 is framed but its meaning remains unsupported.

Numeric line/edge patterns 0–9, caps/joins 0–2 and marker symbols 0–48 are decoded.
A composite pattern replaces pattern/cap/join together. Unknown assignments
replace stale local values and remain unsupported. Text anchor and justification
inherit independently; a no-setting component preserves the prior component.
Unknown text-alignment indices remain unsupported.

Line Style definitions retain their raw source and selected decoded programs:
solid forms; positive finite dash/blank lengths in object-relative (`oru`) or
pixel units; parallel groups with weights/offsets; unsized elements; fixed and
segmented modifiers; contrast, fit and absolute requests. Programs are bounded
to 256 clauses and 32 parallels. Duplicate/ambiguous modifiers and unknown syntax
reject interpretation of the whole program. Omitted weight units remain
unresolved, including with absolute weight. A trailing transformed request
depends on unevaluated Geometry Options and stays unsupported.

Named line/edge patterns can bind a literal ASCII name to one exact, unique
definition at the file import root after a complete inventory, including forward
definitions. Redefinitions, case collisions, non-root definitions, missing names
and incomplete scenes remain unsupported. Unsupported definitions can withhold
placements even when unused. No dash geometry or device conversion is generated.

Glyph Definition is supported for 14.50/19.10, with radius 1–127 and signed XY
coordinates −127..127; 0x80 coordinates are unresolved. Programs are bounded to
512 commands and 4096 points. Basic dot/polyline programs can bind named Marker
Symbols through the unique file-root rule. Known fill/color/curve commands retain
operands without applying drawing state; unknown instructions preserve raw
bytes. Line Style glyph elements and general definition scope remain unsupported.
Malformed known instructions error. No glyph geometry is rendered.

## Views, rendering events and textures

`views` preserves ordered named camera presets, including duplicate names.
`cameras` preserves ordered unnamed camera assignments and their owning nodes.
Neither chooses an active camera, projects geometry or identifies a CAD sheet.
Nonfinite values and invalid field dimensions error; degenerate bases and
unknown name encodings retain explicit unsupported status.

`rendering_options` retains ordered settings and operand spans. Selected Font,
Heuristics and Rendering Options fields expose typed values, flags and enums.
Decoding status is distinct from interpretation: font selection, locks, reset
semantics, clipping, palettes, hidden-line/shadow effects and other unevaluated
state can withhold placements. Format-0 Font data stays opaque. User indices
retain separate signed index/value arrays without application semantics.

`textures` preserves names, image references, raw flags and decoded basic option
values. Missing values remain absent; unknown enums/extended apply modes are not
converted to defaults. `texture_uses` retains ordered diffuse-texture requests,
original expressions, requested names and optional normalized RGB. It is event
history, not final material state.

`TextureBinding` may associate a request with a unique local implicit named image
through the owner and unconditional Style edges. Later definitions are visible
only after a complete inventory. Duplicate providers, case-only collisions and
missing/conditional/cyclic/conflicting styles remain unsupported. Parent,
Include and global lookup are not inferred. A matching Texture definition is
option evidence, not evaluated runtime state.

`TextureDefinition.image_name_matches` is a raw-byte name correlation over the
consumed prefix, not a runtime binding. `images` retains encoded resources and
their owner, reference point, dimensions, names and JPEG/alpha spans under the
[experimental image profile](integrations.md#experimental-images).
There is no pixel decoding, external loading or final material evaluation.

[Resource limits and serialization](resource-limits.md) ·
[Documentation index](README.md)
