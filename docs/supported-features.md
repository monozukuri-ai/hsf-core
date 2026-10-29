# Supported features and limitations

Syntax profiles are selected for HSF **8.05, 14.50 and 19.10**. Availability within
a profile depends on record flags and operands. Unknown framing stops at the
original opcode without searching for later markers. Use `capabilities()` for
the current numeric opcode list and [Getting started](getting-started.md) for
strict/partial behavior.

## Syntax and compression

Supported structural records include termination, comments, file/bounding
information, segment open/close/reopen, includes, styles, references, implicit
tags, user options/values/indices, Pause and start/stop compression. Some records
are framed even when their scene semantics remain unsupported.

Multiple alternating compressed/uncompressed blocks are supported. Internal
compression uses zlib; nested compression is unsupported. Stop Compression must
end a fully parsed compressed domain, and termination must end the outer file.
Pause preserves segment/tag state and does not substitute for termination.

## Geometry and text

| Feature | Supported result | Boundary |
| --- | --- | --- |
| Shell | Uncompressed f32 points; CS_TRIVIAL 8/16/32-bit face values, including signed strips/fans | Quantized points, connectivity compression, collections and bounding-only Shells are unsupported |
| Shell triangulation | Triangles, convex planar quads, strips and fans, with source-face indices | Holes, larger polygons and non-convex/non-planar/degenerate quads retain arrays but withhold all triangles for that mesh |
| Rectangular Mesh | Uncompressed row-major points, flags 0/8, rows and columns at least 2; two triangles per cell | Other storage profiles and unknown attributes stop framing |
| Null Shell | Placeholder identity plus a unique uncompressed LOD 0 refinement of an already tagged LOD 0 Null Shell | Duplicate updates revoke the binding; other targets and general LOD selection remain unsupported |
| Lines and markers | Line, Polyline and Marker local coordinates | Compressed polyline profiles are unsupported |
| PolyPolyline | Uncompressed batches, component lengths and unrestricted/zero/shared/per-component axes | At least one unrestricted axis is required; compression and single-component omission are unsupported |
| Polygon | Ordered vertices with implicit closure, at least three finite points | No appended closing vertex, triangulation or planarity/simple-boundary claim |
| Circle and arc | Three stored control points and an optional explicit center | No fitting, tessellation or projection; explicit flags/centers require 12.15+ |
| Text | Local anchor, raw bytes and supported decoded text | Text options require 10.02+; no font selection or glyph layout |

Text supports plain ASCII and explicit ISO-8859-1 (code 0), UTF-16LE (5),
UTF-32LE (6) and UTF-8 (10). Unknown encodings retain bytes with unsupported text
status; malformed Unicode errors without replacement. Text is not normalized and
BOM/NUL values are not stripped. Raw User Options without a verified encoding
have a separate text status and do not alone make generic geometry incomplete.
Text Region stores up to four finite points and raw controls, but region layout
is unevaluated and withholds placements.

## Stored mesh attributes

Full-resolution Shell/Mesh parameters support tuples of **1–128 components**,
with a separate flat `count × width` buffer. Supported encodings are uncompressed
f32 and selected CS_TRIVIAL quantization: Shell uses 2–31 bits per sample; Mesh
uses 8. Width-three/eight-bit samples use direct bytes without word padding;
other quantized tuples use MSB-first fields in little-endian 32-bit words.
Bounds and packed source spans are preserved. Interpolation uses f64 then f32
rounding with exact endpoints; original pre-quantization values are not recovered.

Stored parameters have no inferred UVW, color or spatial meaning. No clamping,
V flip, generated UVs or texture-matrix application occurs. Refinement/nonzero-LOD
parameter sets preserve their own vertex order and prior-tag association; they
do not imply alignment to base vertices or geometry replacement. The bounded
Null Shell update above is the supported geometry-binding exception.

Uncompressed all-vertex normals are retained. Shell compressed normals support
schemes 1 (XYZ) and 7 (polar), 2–31 bits, producing local f32 vectors with source
and quantization metadata. Polar inclination is measured from +Y. Normals are
not normalized or transformed. Entity User/Unicode Options preserve ordered
fields and decode ASCII/strict UTF-16LE when applicable; product meanings remain
unsupported. Unknown optional attributes still stop at the enclosing opcode.

## Scene and resource interpretation

| Feature | Scope |
| --- | --- |
| Segments and references | Bounded literal paths, optionally rebased through a configured mount path, previously known tags, includes/references (Include Library targets render only through includes), reopen attributes, Repeat Object copies and in-place Null Shell instances |
| Transforms and handedness | Stored modelling matrices and inherited handedness, with local geometry preserved |
| Display attributes | Selected RGB/visibility masks, size/weight units, basic numeric patterns/symbols, text alignment and texture matrices |
| Style Segment | Bounded unconditional display inheritance; conditions, conflicts, cycles and unknown effects remain unsupported |
| Custom line styles and glyphs | Selected stored programs and unique file-root bindings; no generated dash/glyph geometry |
| Views and cameras | Ordered metadata; no active-camera selection or projection; 8.05 near-limit framing is unsupported |
| Textures | Definitions, explicit diffuse requests, option decoding and bounded image associations; no final material evaluation |
| Rendering settings | Framed source-backed events and selected typed operands; unevaluated effects withhold placements |
| Images and User Data | Explicit experimental inventory only; disabled by default and rejected in strict mode |

See [Scene data](scene-data.md) for interpretation rules and
[Adapter integration](integrations.md) for experimental profiles.

## Remaining boundaries

This crate does not read outer application containers or infer CAD sheets,
components, configurations, dimension meaning or units. It does not write HSF,
render scenes, load external databases/images/fonts, evaluate clipping/palettes,
or recover final material state. Parsing and synthetic fixture validation do not
establish native application or Viewer compatibility.

Unsupported global state can withhold every placement while retaining local
geometry. Missing or unsupported reference branches may be omitted with explicit
diagnostics. Never treat a partial scene or a complete syntax result as proof
of complete document interpretation.

`SceneOptions.placement = PlacementPolicy::BestEffort` is the one exception to
withholding: in partial mode it traverses the stored tree, matrices and
references anyway and reports `hsf.placement_estimated` with the ignored state.
Those instances are estimates, not verified placements; strict mode rejects the
policy. See [Scene data](scene-data.md#placement-policy).
