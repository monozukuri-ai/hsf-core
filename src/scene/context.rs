//! Explicit caller-provided namespace for inventory, not an imported database.
use super::*;

#[derive(Clone, Debug, Serialize)]
pub struct SceneImportContext {
    pub requested_root: String,
    pub resolved_root: String,
    pub node_id: String,
    /// Nodes introduced by the caller context, including the modeled global root.
    /// They have no file source and do not prove external attributes or geometry.
    pub context_node_ids: Vec<String>,
    pub interpretation_status: Status,
}

impl Builder<'_> {
    pub(super) fn initialize_context(&mut self) -> Result<(), HsfError> {
        let Some(requested) = &self.options.import_root else {
            return Ok(());
        };
        let invalid = || {
            let mut e = error(
                "hsf.invalid_import_root",
                Status::Invalid,
                "Import root must be an absolute unquoted ASCII literal path of at most 4096 bytes; empty or navigation components, aliases and wildcards are forbidden",
            );
            e.diagnostic.stage = "scene".into();
            e
        };
        if requested.len() > 4096
            || !requested.starts_with('/')
            || requested
                .bytes()
                .any(|b| !(b.is_ascii_alphanumeric() || b" /#+-$_ .".contains(&b)))
        {
            return Err(invalid());
        }
        self.budget
            .charge_output(2048 + requested.len() as u64 * 16)?;
        let components: Vec<String> = if requested == "/" {
            vec![]
        } else {
            requested[1..]
                .split('/')
                .map(|s| {
                    s.split(' ')
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .to_ascii_lowercase()
                })
                .collect()
        };
        if components
            .iter()
            .any(|s| matches!(s.as_str(), "" | "." | ".." | "..." | "...."))
        {
            return Err(invalid());
        }
        limit(
            components.len() as u64 + 1,
            self.options.max_depth,
            "hsf.limit_scene_depth",
        )?;
        let mut d = error(
            "hsf.import_context",
            Status::Unsupported,
            "Caller-supplied import root used for inventory; external state is not loaded and all placements are withheld",
        );
        d.diagnostic.stage = "scene".into();
        d.diagnostic.version = Some(self.doc.header.version.text.clone());
        if self.options.mode == ParseMode::Strict {
            return Err(d);
        }
        self.budget.charge_output(512)?;
        d.diagnostic.severity = "warning".into();
        self.diagnostics.push(*d.diagnostic);
        let mut parent = 0;
        let mut path = String::new();
        for component in components {
            path.push('/');
            path.push_str(&component);
            self.budget.charge_output(128 + path.len() as u64 * 2)?;
            parent = self.node(Some(parent), Some(component), None, None)?;
            self.paths.insert(path.clone(), parent);
            self.node_paths[parent] = Some(path.clone());
        }
        if path.is_empty() {
            path.push('/');
        }
        self.import_node = parent;
        self.initial_nodes = self.data.nodes.len();
        self.stack[0] = parent;
        self.budget.charge_output(self.initial_nodes as u64 * 128)?;
        self.data.import_context = Some(SceneImportContext {
            requested_root: requested.clone(),
            resolved_root: path,
            node_id: self.data.nodes[parent].id.clone(),
            context_node_ids: self.data.nodes.iter().map(|n| n.id.clone()).collect(),
            interpretation_status: Status::Unsupported,
        });
        Ok(())
    }
}
