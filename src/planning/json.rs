use crate::spec::{ExternalTypeSpec, JsonModelSpec};

use super::ApiPlanner;

impl ApiPlanner<'_> {
    pub(super) fn mark_json_model_used(&mut self, json_type: &JsonModelSpec<crate::spec::Symbol>) {
        if !self
            .used_json_models
            .insert(json_type.name.as_str().to_string())
        {
            return;
        }
        self.mark_json_schema_refs_used(&json_type.schema);
    }

    fn mark_json_schema_refs_used(&mut self, schema: &serde_json::Value) {
        let Some(object) = schema.as_object() else {
            return;
        };

        if let Some(reference) = object.get("$ref").and_then(serde_json::Value::as_str)
            && let Some(model_name) = json_ref_model_name(reference)
            && let Some(nested) = self.json_model_for_ref(&model_name)
        {
            self.mark_json_model_used(&nested);
        }

        for value in object.values() {
            match value {
                serde_json::Value::Array(values) => {
                    for value in values {
                        self.mark_json_schema_refs_used(value);
                    }
                }
                serde_json::Value::Object(_) => self.mark_json_schema_refs_used(value),
                _ => {}
            }
        }
    }

    fn json_model_for_ref(&self, model_name: &str) -> Option<JsonModelSpec<crate::spec::Symbol>> {
        self.spec
            .external_types
            .get(model_name)
            .and_then(|binding| match &binding.external_type {
                ExternalTypeSpec::Json(json_type) => Some(json_type.clone()),
                _ => None,
            })
            .or_else(|| {
                self.spec
                    .external_types
                    .values()
                    .find_map(|binding| match &binding.external_type {
                        ExternalTypeSpec::Json(json_type)
                            if json_type.model_name == model_name
                                || json_type.name.local_name() == model_name =>
                        {
                            Some(json_type.clone())
                        }
                        _ => None,
                    })
            })
    }
}

fn json_ref_model_name(reference: &str) -> Option<String> {
    let fragment = reference
        .split_once('#')
        .map(|(_, fragment)| fragment)
        .unwrap_or(reference);
    let name = fragment
        .strip_prefix("/$defs/")
        .or_else(|| fragment.trim_start_matches('/').rsplit('/').next())?;
    let name = name.replace("~1", "/").replace("~0", "~");
    (!name.is_empty()).then_some(name)
}
