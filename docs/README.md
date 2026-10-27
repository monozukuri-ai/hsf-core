# Documentation

`hsf-core` separates HSF syntax framing from generic scene interpretation. Start
with the usage guide, then check feature-specific limits before consuming data.

| Guide | Contents |
| --- | --- |
| [Getting started](getting-started.md) | Source dependency, file input, parsing, scenes and errors |
| [Supported features](supported-features.md) | Version profiles, supported geometry and explicit limitations |
| [Scene data](scene-data.md) | Definitions, instances, transforms, display state and resource inventories |
| [Resource limits](resource-limits.md) | Byte budgets, object limits, source domains and serialization |
| [Adapter integration](integrations.md) | Precharged input, build identity and experimental profiles |

For the full type and field reference, run `cargo doc --no-deps --open` from a
source checkout. Public APIs are exported at the `hsf_core` crate root.

[Project overview](../README.md) · [Changelog](../CHANGELOG.md) ·
[License](../LICENSE) · [Notice](../NOTICE)
