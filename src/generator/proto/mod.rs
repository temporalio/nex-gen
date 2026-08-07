pub(crate) mod dotnet;
pub(crate) mod go;
pub(crate) mod python;
pub(crate) mod typescript;

use crate::error::{Error, Result};
use crate::language::Language;
use crate::planning::PlannedFamily;
use crate::spec::{ApiSpecNode, ApiSpecTree, RecordFieldVisibility};

/// Reject visible, wire-converted protobuf oneofs for backends that do not yet
/// know how to render their conversion. This runs on reachability-pruned IR so
/// unused declarations and omitted oneofs remain harmless.
pub(crate) fn ensure_supported_oneof_conversions(
    tree: &ApiSpecTree<PlannedFamily>,
    language: Language,
) -> Result<()> {
    fn scan(node: &ApiSpecNode<PlannedFamily>, language: Language) -> Result<()> {
        match node {
            ApiSpecNode::Leaf(leaf) => {
                for (_, record) in leaf.spec.records() {
                    if !record.data.capabilities.from_wire && !record.data.capabilities.to_wire {
                        continue;
                    }
                    for field in record
                        .fields
                        .values()
                        .filter(|field| field.visibility != RecordFieldVisibility::Omitted)
                    {
                        let Some(oneof) = &field.data.oneof else {
                            continue;
                        };
                        return Err(Error::UnsupportedProtoOneofConversion {
                            language,
                            message: record
                                .data
                                .proto
                                .as_ref()
                                .map(|proto| proto.full_name.clone())
                                .unwrap_or_else(|| record.full_name.clone()),
                            oneof: oneof.name.clone(),
                        });
                    }
                }
                Ok(())
            }
            ApiSpecNode::Branch(branch) => {
                for child in branch.children.values() {
                    scan(child, language)?;
                }
                Ok(())
            }
        }
    }

    scan(&tree.root, language)
}
