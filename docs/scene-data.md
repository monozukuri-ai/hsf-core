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

### Placement policy

`SceneOptions.placement` defaults to `PlacementPolicy::Verified`: whenever opaque
User Data, unresolved refinements or instances, unevaluated rendering settings,
textures, images, import context, startup aliases or incomplete style
resolutions remain, every placement is withheld and `placement_status` is
`unsupported`. `PlacementPolicy::BestEffort` traverses the stored tree, matrices
and references anyway (also after a partial stop, for the processed prefix) and
adds the warning `hsf.placement_estimated`, whose message lists the ignored
state. The result is always partial and strict mode rejects the policy
(`hsf.placement_policy_rejected`). Treat those instances as estimates to verify
against the producing application, never as proof of arrangement.

`SceneOptions.traversal` chooses what the traversal places. `All` (default)
walks everything reachable from the file root. `Subtree("/path")` places only
the subtree at that absolute literal segment path plus whatever it includes or
references: ancestors contribute inherited attributes but place nothing of
their own, sibling branches place nothing, and nodes, definitions and
inventories still cover the whole file. The info diagnostic
`hsf.traversal_subtree` records the choice, so the result is partial; an unknown
path places nothing and reports `hsf.traversal_root_unknown` (an error in strict
mode). `Skip` builds the inventory without any placement. Producers that save
several configurations as root children (eDrawings `configN`) are read one
configuration at a time this way instead of placing every configuration.
`user_option_text()` decodes a `U`/Unicode Options syntax record the way the
scene does, for callers that select the subtree before building the scene.

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
and `.`/`..`/`^` navigation. A component wrapped in backquotes is a literal
name: the quotes are not part of it, its spacing is kept, and navigation, alias
and wildcard meanings do not apply inside; a literal that would read as
navigation is stored re-quoted. Source spelling is retained. Open Segment can
create missing literal parents with `implicit = true` and the creating record's
source; Close returns to the original scope. Anonymous segments are supported,
but relative paths from them are not. Slashes or nested quotes inside quotes,
text glued to a quote, wildcards, path lists, unknown aliases and non-ASCII
names remain unsupported.

Paths that climb above the file root are unsupported unless
`SceneOptions.mount_path` names the absolute runtime path at which the producer
mounted this file's root. Navigation then continues through the mount's
ancestors: a result inside the mount maps back to a file path (an open creates
or reuses that file segment), and a result outside it is external. External
opens stop with `hsf.segment_path_external`; external includes and styles omit
their branch (`hsf.reference_external`). The mount path follows the import-root
grammar and errors with `hsf.invalid_mount_path`. Nothing outside the file is
loaded or asserted.

References resolve previously known literal paths/tags. Reopen uses final segment
attributes. Repeat Object (`TK_Instance` order: source index/variant, destination
index/variant, options, matrix) supports variant 0 and the tristrip option bit.
With destination `-1` it copies previously tagged geometry into the current
segment. With a destination index it makes an existing tagged Null Shell an
in-place instance of a tagged Null Shell source: `NullShell.instance_of` and
`instance_transform` record the link, the copy keeps its own segment, and its
geometry is the source's bound LOD 0 geometry times the instance matrix once
that source is bound (before or after the record). Unbound sources, destinations
with their own update and other target kinds are explicit
(`hsf.repeat_source_unbound`, `hsf.repeat_target_bound`, `hsf.repeat_target`,
`hsf.repeat_variant`). Unresolved references, cycles and conditional includes
omit their branch with diagnostics; unsupported scope can withhold all placements.
Traversal depth/work remains bounded even when expansion yields no geometry.

Segments under the standard Include Library (`/include library`, by alias,
literal path or mount path) are definitions: when they are include targets they
render only through their includes, not at their own tree position. Included
segments elsewhere render both at their position and through the include, as in
HOOPS. Library segments that nothing includes are traversed normally.

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
