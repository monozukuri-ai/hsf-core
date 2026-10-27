# Resource limits and source provenance

## Shared byte budget

Pass one `Budget` through outer-container extraction, HSF parsing and scene
generation when those operations belong to one import. `Limits` defaults are:

| Field | Default | Charged work |
| --- | --- | --- |
| `max_input_bytes` | 256 MiB | Input bytes |
| `max_expanded_bytes` | 512 MiB | Decompressed bytes |
| `max_output_bytes` | 768 MiB | Retained data and conservative allocation charges |

Zero permits no consumption; it never means unlimited. Counters are cumulative,
check overflow before charging and do not refund released buffers. `usage()` and
each outcome's `usage` expose cumulative totals at that point in the operation.

`parse` charges input and its retained copy. `parse_precharged` requires the caller
to account for input ownership and any outer decompression beforehand; see
[Adapter integration](integrations.md#precharged-input). Caller buffers, later
serialization and unrelated application allocations are not automatically
accounted for. Bound file/container reads before allocating them.

Output accounting includes payloads and conservative metadata charges (including
2048 bytes per syntax record and 512 per domain), along with geometry, temporary
buffers, display copies and traversal workspaces. It is an import budget, not a
process RSS limit or a downstream serialization-size guarantee.

## Parser limits

`ParseOptions` counters apply to one parse, independently of the shared byte
budget. Values below are defaults; lower them for a narrower workload.

| Field | Default | Scope |
| --- | --- | --- |
| `max_records` | 1,000,000 | Syntax records; also bounds User Index counts |
| `max_segment_depth` | 256 | Encoded segment nesting |
| `max_domains` | 128 | Retained input/decoded domains |
| `max_string_bytes` | 16 MiB | Bounded strings and opaque payloads |
| `max_compression_ratio` | 1,000 | Internal zlib expansion ratio |
| `max_vertices` | 10,000,000 | Cumulative geometry points, including primitive/clip/text-region points and explicit centers |
| `max_face_values` | 50,000,000 | Cumulative face-list values |
| `max_image_pixels` | 16,777,216 | Cumulative pixels for the opt-in image profile |

Unknown flags and unsupported storage profiles stop parsing. Truncated known
operands, invalid counts/coordinates and exhausted limits error in both modes.

## Scene limits

| `SceneOptions` field | Default | Scope |
| --- | --- | --- |
| `max_meshes` | 100,000 | Meshes, Null Shell placeholders and refinement parameter inventories |
| `max_primitives` | 100,000 | Primitives, including PolyPolyline component counts |
| `max_nodes` | 100,000 | Nodes, including implicit and import-context ancestors |
| `max_instances` | 1,000,000 | Mesh and primitive placements combined |
| `max_depth` | 256 | Node/reference/style depth |
| `max_visits` | 2,000,000 | Shared geometry/style traversal work |
| `max_triangles` | 10,000,000 | Generated triangles |
| `max_views` | 10,000 | Named views and segment cameras combined |
| `max_definitions` | 10,000 | Combined bounded definitions and alias/style metadata |

Definition resources include line styles, fonts, glyphs, textures and images.
Traversal caps also apply to acyclic references that produce no geometry. Limits
are checked alongside byte-budget charges, not replaced by them. Fixed profile
caps, such as parameter width, glyph-program size and adapter field counts, are
described in the [support](supported-features.md), [scene](scene-data.md) and
[integration](integrations.md) guides.

## Domains and spans

`HsfDocument.domains` retains the original input and every decoded domain. Each
`SourceDomain` exposes its ID, SHA-256, byte length, encoding, contiguous parsed
prefix length and an optional parent compressed span.

`SourceSpan { domain_id, offset, length }` addresses bytes within exactly one
domain. Decoded offsets and parent compressed offsets must never be added
together. Use `HsfDocument::source_bytes(&span)` for checked access; it returns
`None` for invalid domain/range combinations. `SourceDomain::bytes()` exposes
the retained domain directly.

A parsed outer compressed payload does not imply complete interpretation of
its decoded domain. `ParseOutcome.unparsed` records unparsed ranges by domain;
diagnostics and `Scene.processed_records` carry the separate semantic boundary.

## Serialization

Serde serialization exposes metadata and source references, but skips retained
domain bytes and bulk geometry vectors. Mesh vertices, face lists, triangles,
triangle-face indices, normals and parameters remain native Rust vectors.
Primitive points and Shell parameter-set values are also skipped.

Serialize those arrays explicitly in an adapter if needed, with a separate
output-size policy. A metadata JSON value alone cannot reconstruct the full
retained source or geometry.

[Getting started](getting-started.md) · [Documentation index](README.md)
