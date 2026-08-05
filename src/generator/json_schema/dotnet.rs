use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use heck::ToUpperCamelCase;
use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Number, Value};

use crate::error::{Error, Result};
use crate::generator::ExternalModelBackend;
use crate::generator::dotnet::{
    WireValueConversion, csharp_parameter_name, csharp_string_literal, csharp_type_name,
};
use crate::planning::{PlannedJsonType, PlannedSpec, PlannedTypeFamily};
use crate::spec::{ExternalTypeSpec, RecordSpec};

const GENERATED_CODE_ATTRIBUTE: &str = "[GeneratedCode(\"nex-gen\", null)]";

#[derive(Debug, Deserialize, Default)]
struct Schema {
    #[serde(rename = "$ref")]
    reference: Option<String>,
    #[serde(rename = "type")]
    ty: Option<Value>,
    description: Option<String>,
    properties: Option<IndexMap<String, Schema>>,
    required: Option<Vec<String>>,
    #[serde(rename = "additionalProperties")]
    additional_properties: Option<Value>,
    items: Option<Box<Schema>>,
    #[serde(rename = "oneOf")]
    one_of: Option<Vec<Schema>>,
    default: Option<Value>,
    #[serde(rename = "const")]
    const_value: Option<Value>,
    #[serde(rename = "maxProperties")]
    max_properties: Option<usize>,
    // Numeric bounds. Kept as `serde_json::Number` so an integral bound renders
    // without a spurious `.0` and a fractional one keeps its precision, matching
    // how Go's `%v` prints the same bound.
    minimum: Option<Number>,
    maximum: Option<Number>,
    #[serde(rename = "exclusiveMinimum")]
    exclusive_minimum: Option<Number>,
    #[serde(rename = "exclusiveMaximum")]
    exclusive_maximum: Option<Number>,
    #[serde(rename = "multipleOf")]
    multiple_of: Option<Number>,
    #[serde(rename = "minLength")]
    min_length: Option<usize>,
    #[serde(rename = "maxLength")]
    max_length: Option<usize>,
    pattern: Option<String>,
    #[serde(rename = "minItems")]
    min_items: Option<usize>,
    #[serde(rename = "maxItems")]
    max_items: Option<usize>,
    #[serde(rename = "uniqueItems")]
    unique_items: Option<bool>,
    contains: Option<Box<Schema>>,
    #[serde(rename = "minContains")]
    min_contains: Option<usize>,
    #[serde(rename = "maxContains")]
    max_contains: Option<usize>,
    #[serde(rename = "minProperties")]
    min_properties: Option<usize>,
    #[serde(rename = "propertyNames")]
    property_names: Option<Box<Schema>>,
    #[serde(rename = "dependentRequired")]
    dependent_required: Option<IndexMap<String, Vec<String>>>,
    #[serde(rename = "enum")]
    enum_values: Option<Vec<Value>>,
}

impl Schema {
    /// The numeric bounds declared on this schema, in the order Go emits them so
    /// a multi-violation payload lists them identically across targets.
    fn numeric_bounds(&self) -> Vec<NumericBound<'_>> {
        [
            (NumericBoundKind::Minimum, self.minimum.as_ref()),
            (NumericBoundKind::Maximum, self.maximum.as_ref()),
            (
                NumericBoundKind::ExclusiveMinimum,
                self.exclusive_minimum.as_ref(),
            ),
            (
                NumericBoundKind::ExclusiveMaximum,
                self.exclusive_maximum.as_ref(),
            ),
            (NumericBoundKind::MultipleOf, self.multiple_of.as_ref()),
        ]
        .into_iter()
        .filter_map(|(kind, bound)| bound.map(|bound| NumericBound { kind, bound }))
        .collect()
    }

    /// The string-length bounds declared on this schema, `minLength` first to
    /// match the order Go and Java emit them in.
    fn length_bounds(&self) -> Vec<LengthBound> {
        [(true, self.min_length), (false, self.max_length)]
            .into_iter()
            .filter_map(|(at_least, bound)| bound.map(|bound| LengthBound { at_least, bound }))
            .collect()
    }

    /// The array-length bounds declared on this schema, `minItems` first.
    fn item_count_bounds(&self) -> Vec<ItemCountBound> {
        [(true, self.min_items), (false, self.max_items)]
            .into_iter()
            .filter_map(|(at_least, bound)| bound.map(|bound| ItemCountBound { at_least, bound }))
            .collect()
    }

    /// The `contains` check, when it is a shape .NET can lower.
    ///
    /// Only a bare `const` branch is supported, which is what the corpus uses and
    /// all Go emits — matching an arbitrary subschema per element would need the
    /// whole validator to be reentrant over element values. Anything else stays a
    /// reported gap; see [`contains_is_supported`].
    fn contains_check(&self) -> Option<ContainsCheck> {
        let contains = self.contains.as_deref()?;
        let literal = contains
            .const_value
            .as_ref()
            .and_then(csharp_value_literal)?;
        Some(ContainsCheck {
            literal,
            // `contains` without `minContains` means "at least one" per the spec.
            min: self.min_contains.unwrap_or(1),
            max: self.max_contains,
        })
    }

    /// The object-level constraints declared on this schema.
    ///
    /// These are checked against the **wire member set** rather than any single
    /// member's value, which is why they render at the top level of
    /// `CollectViolations` rather than inside a member guard.
    fn object_constraints(&self) -> Result<ObjectConstraints> {
        // `propertyNames` is lowered only for a map-shaped object, whose extension
        // bag holds every wire member. On an object with declared properties the
        // keyword also governs the declared names, which the bag does not carry.
        let property_names = match (&self.property_names, self.has_declared_properties()) {
            (Some(names), false) => names.length_bounds(),
            _ => Vec::new(),
        };
        Ok(ObjectConstraints {
            count_bounds: [(true, self.min_properties), (false, self.max_properties)]
                .into_iter()
                .filter_map(|(at_least, bound)| {
                    bound.map(|bound| PropertyCountBound { at_least, bound })
                })
                .collect(),
            property_name_lengths: property_names,
            dependent_required: self
                .dependent_required
                .as_ref()
                .map(|dependencies| {
                    dependencies
                        .iter()
                        .flat_map(|(trigger, dependents)| {
                            dependents.iter().map(move |dependent| DependentRequired {
                                trigger: trigger.clone(),
                                dependent: dependent.clone(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// The closed value set this schema admits, as the C# literals to compare
    /// against plus the bracketed list Go names in its violation reason.
    fn enum_membership(&self) -> Option<EnumMembership> {
        let values = self
            .enum_values
            .as_ref()
            .filter(|values| !values.is_empty())?;
        let literals = values
            .iter()
            .map(csharp_value_literal)
            .collect::<Option<Vec<_>>>()?;
        // Rendered the way Go's `%v` over the decoded values does: strings quoted,
        // numbers bare, comma-separated with no spaces.
        let rendered = values
            .iter()
            .map(|value| match value {
                Value::String(text) => format!("{text:?}"),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join(",");
        Some(EnumMembership {
            literals,
            rendered,
            quotes_value: values.iter().any(|value| value.is_string()),
        })
    }

    /// True when the member declares a closed value set, which makes the spec
    /// integer cap redundant — membership already bounds the value. Go skips the
    /// cap on these for the same reason.
    fn has_closed_value_set(&self) -> bool {
        self.const_value.is_some()
            || self
                .enum_values
                .as_ref()
                .is_some_and(|values| !values.is_empty())
    }

    fn has_declared_properties(&self) -> bool {
        self.properties
            .as_ref()
            .is_some_and(|properties| !properties.is_empty())
    }

    /// This schema's `pattern` with its `$` end anchor rewritten to `\z`.
    ///
    /// .NET's `Regex` treats a bare `$` as "end of string, or before a single
    /// trailing newline" — the same exception Python and Java have — so a value
    /// ending in `\n` would pass a `$`-anchored pattern that the contract intends
    /// to reject. `\z` is the unconditional end-of-input anchor. Go and JS keep
    /// `$` because their engines have no such exception.
    fn dotnet_pattern(&self) -> Option<String> {
        self.pattern
            .as_deref()
            .map(|pattern| crate::json_schema::pattern::rewrite_end_anchor(pattern, r"\z"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NumericBoundKind {
    Minimum,
    Maximum,
    ExclusiveMinimum,
    ExclusiveMaximum,
    MultipleOf,
}

#[derive(Debug)]
struct NumericBound<'a> {
    kind: NumericBoundKind,
    bound: &'a Number,
}

impl NumericBound<'_> {
    /// The C# boolean expression that is true when `value_expr` **violates** the
    /// bound.
    fn violation_condition(&self, value_expr: &str) -> String {
        let bound = self.bound;
        match self.kind {
            NumericBoundKind::Minimum => format!("{value_expr} < {bound}"),
            NumericBoundKind::Maximum => format!("{value_expr} > {bound}"),
            NumericBoundKind::ExclusiveMinimum => format!("{value_expr} <= {bound}"),
            NumericBoundKind::ExclusiveMaximum => format!("{value_expr} >= {bound}"),
            // `%` on a double is exact for the values the spec-number cap admits,
            // and `multipleOf` bounds are themselves exact in binary far more
            // often than not; a remainder test matches Go's `math.Mod` check.
            NumericBoundKind::MultipleOf => format!("{value_expr} % {bound} != 0"),
        }
    }

    /// The violation reason, worded exactly as Go's equivalent so the same
    /// payload produces the same diagnostic text on every target.
    fn reason_format(&self) -> String {
        let bound = self.bound;
        match self.kind {
            NumericBoundKind::Minimum => format!("must be >= {bound}, got "),
            NumericBoundKind::Maximum => format!("must be <= {bound}, got "),
            NumericBoundKind::ExclusiveMinimum => format!("must be > {bound}, got "),
            NumericBoundKind::ExclusiveMaximum => format!("must be < {bound}, got "),
            NumericBoundKind::MultipleOf => format!("must be a multiple of {bound}, got "),
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::generator) struct ModelBackend {
    json_models: Vec<PlannedJsonType>,
}

#[derive(Debug, Default)]
pub(in crate::generator) struct RenderedModelFragments {
    pub(in crate::generator) body: String,
}

impl RenderedModelFragments {
    pub(in crate::generator) fn has_models(&self) -> bool {
        !self.body.is_empty()
    }

    /// True when any emitted model compiles a `pattern`, so the models file needs
    /// `System.Text.RegularExpressions`.
    pub(in crate::generator) fn needs_regex(&self) -> bool {
        self.body.contains("new Regex(")
    }
}

impl ExternalModelBackend<PlannedJsonType> for ModelBackend {
    type ModelFragments = RenderedModelFragments;
    type WireConversion = WireValueConversion;

    fn prepare(&mut self, api_plan: &PlannedSpec) -> Result<()> {
        self.json_models = api_plan
            .external_types()
            .map(|(_, binding)| binding)
            .filter_map(|binding| match &binding.external_type {
                ExternalTypeSpec::Json(json_type) => Some(json_type.clone()),
                _ => None,
            })
            .collect();
        Ok(())
    }

    fn render_models(&self) -> Result<RenderedModelFragments> {
        render_external_models(&self.json_models)
    }

    fn model_type_annotation(&self, json_type: &PlannedJsonType) -> Option<String> {
        Some(model_type_ref(json_type))
    }

    fn wire_type_identifier(&self, json_type: &PlannedJsonType) -> Option<String> {
        Some(json_type.full_name.clone())
    }

    fn wire_conversion(
        &self,
        json_type: &PlannedJsonType,
        _planned_record: Option<&RecordSpec<PlannedTypeFamily>>,
    ) -> Option<WireValueConversion> {
        Some(WireValueConversion {
            annotation: model_type_ref(json_type),
            to_wire: "{value}".to_string(),
        })
    }
}

fn model_type_ref(json_type: &PlannedJsonType) -> String {
    csharp_type_name(&json_type.model_name)
}

/// A closed sum type: `oneOf` over `$ref` branches that all share a required
/// `const` discriminator member.
///
/// Lowered to an abstract base class plus a `JsonConverter` that reads the tag and
/// routes to the branch. Before this, the union rendered as a class with no
/// members at all and both branches were simply lost.
#[derive(Debug)]
struct TaggedUnion {
    /// The JSON member carrying the tag, e.g. `kind`.
    discriminator: String,
    /// `(tag value, branch type name)` in declaration order.
    branches: Vec<(String, String)>,
}

impl TaggedUnion {
    /// The bracketed tag list Go names when no branch matches. Note the
    /// comma-**space** separator, which differs from the `enum` reason's list.
    fn expected_tags(&self) -> String {
        self.branches
            .iter()
            .map(|(tag, _)| format!("{tag:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn converter_name(base: &str) -> String {
        format!("{base}JsonConverter")
    }
}

/// Every tagged union in one emitted package, plus the reverse branch index.
#[derive(Debug, Default)]
struct TaggedUnions {
    definitions: BTreeMap<String, TaggedUnion>,
    /// Branch type name to its base class.
    branch_bases: BTreeMap<String, String>,
}

impl TaggedUnions {
    fn resolve(json_models: &[PlannedJsonType]) -> Result<Self> {
        // Branch schemas are looked up by emitted type name, which is how a `$ref`
        // resolves once the name manifest has been applied.
        let mut schemas = BTreeMap::new();
        for model in json_models {
            schemas.insert(model_type_ref(model), decode_schema(model)?);
        }

        let mut resolved = Self::default();
        for model in json_models {
            let type_name = model_type_ref(model);
            let schema = &schemas[&type_name];
            let Some(union) = tagged_union_for(schema, &schemas) else {
                continue;
            };
            for (_, branch) in &union.branches {
                // C# has single inheritance, so a branch shared between two unions
                // cannot be modeled this way. Leave every union unlowered rather
                // than emit something that will not compile; `dotnet_coverage` then
                // reports them all as gaps.
                if resolved.branch_bases.contains_key(branch) {
                    return Ok(Self::default());
                }
                resolved
                    .branch_bases
                    .insert(branch.clone(), type_name.clone());
            }
            resolved.definitions.insert(type_name, union);
        }
        Ok(resolved)
    }

    fn definition(&self, type_name: &str) -> Option<&TaggedUnion> {
        self.definitions.get(type_name)
    }

    fn base_of(&self, type_name: &str) -> Option<&str> {
        self.branch_bases.get(type_name).map(String::as_str)
    }

    /// True when this model is a union branch, which forces its validator to be
    /// emitted (as an override) even when it has no constraints of its own.
    fn is_branch(&self, type_name: &str) -> bool {
        self.branch_bases.contains_key(type_name)
    }
}

/// Emits a tagged union: the abstract base class and the `JsonConverter` that
/// selects a branch from the discriminator.
///
/// The converter is hand-rolled rather than using `[JsonPolymorphic]`, because
/// System.Text.Json's built-in discriminator is a metadata property distinct from
/// the model's own members. Here the tag *is* a declared member — each branch has
/// `kind` as a required `const` — and the two mechanisms collide.
fn render_tagged_union(
    output: &mut String,
    model: &PlannedJsonType,
    union: &TaggedUnion,
) -> Result<()> {
    let schema = decode_schema(model)?;
    let base = model_type_ref(model);
    let converter = TaggedUnion::converter_name(&base);

    render_xml_summary(output, "", schema.description.as_deref());
    output.push_str("[JsonConverter(typeof(");
    output.push_str(&converter);
    output.push_str("))]\n");
    output.push_str(GENERATED_CODE_ATTRIBUTE);
    output.push('\n');
    output.push_str("public abstract class ");
    output.push_str(&base);
    output.push_str("\n{\n");
    // A private-protected constructor closes the hierarchy: only the generated
    // branches in this assembly can derive from it, which is what `oneOf` means.
    output.push_str("    private protected ");
    output.push_str(&base);
    output.push_str("()\n    {\n    }\n\n");
    output.push_str("    /// <summary>\n");
    output.push_str("    /// Validates the selected branch, throwing a single\n");
    output.push_str("    /// <see cref=\"ValidationException\"/> carrying every violation.\n");
    output.push_str("    /// </summary>\n");
    output.push_str("    public abstract void Validate();\n\n");
    output.push_str(
        "    internal abstract void CollectViolations(List<Violation> violations, string path);\n",
    );
    output.push_str("}\n\n");

    output.push_str(GENERATED_CODE_ATTRIBUTE);
    output.push('\n');
    output.push_str("internal sealed class ");
    output.push_str(&converter);
    output.push_str(" : JsonConverter<");
    output.push_str(&base);
    output.push_str(">\n{\n");
    output.push_str("    public override ");
    output.push_str(&base);
    output.push_str(
        " Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)\n    {\n",
    );
    output.push_str("        using var document = JsonDocument.ParseValue(ref reader);\n");
    output.push_str("        var root = document.RootElement;\n");
    output.push_str("        if (root.ValueKind != JsonValueKind.Object)\n        {\n");
    output
        .push_str("            throw new ValidationException(new List<Violation>\n            {\n");
    output.push_str("                new Violation(string.Empty, ");
    output.push_str(&csharp_string_literal(&format!(
        "expected one of: {}",
        union
            .branches
            .iter()
            .map(|(_, branch)| branch.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    )));
    output.push_str("),\n            });\n        }\n");
    output.push_str("        if (!root.TryGetProperty(");
    output.push_str(&csharp_string_literal(&union.discriminator));
    output.push_str(", out var tag))\n        {\n");
    output
        .push_str("            throw new ValidationException(new List<Violation>\n            {\n");
    output.push_str("                new Violation(string.Empty, ");
    output.push_str(&csharp_string_literal(&format!(
        "discriminator {:?} is required",
        union.discriminator
    )));
    output.push_str("),\n            });\n        }\n");
    output.push_str("        var raw = root.GetRawText();\n");
    output.push_str("        switch (tag.ValueKind == JsonValueKind.String ? tag.GetString() : null)\n        {\n");
    for (tag, branch) in &union.branches {
        output.push_str("            case ");
        output.push_str(&csharp_string_literal(tag));
        output.push_str(":\n                return JsonSerializer.Deserialize<");
        output.push_str(branch);
        output.push_str(">(raw, options)!;\n");
    }
    output.push_str("            default:\n");
    output.push_str(
        "                throw new ValidationException(new List<Violation>\n                {\n",
    );
    output.push_str("                    new Violation(string.Empty, $");
    output.push_str(&csharp_string_literal(&format!(
        "unknown discriminator {} {{tag.GetRawText()}}: expected one of [{}]",
        union.discriminator,
        union.expected_tags()
    )));
    output.push_str("),\n                });\n        }\n    }\n\n");
    output.push_str("    public override void Write(Utf8JsonWriter writer, ");
    output.push_str(&base);
    output.push_str(" value, JsonSerializerOptions options)\n    {\n");
    // Dispatching on the runtime type serializes the branch through its own
    // converter rather than re-entering this one.
    output.push_str("        JsonSerializer.Serialize(writer, value, value.GetType(), options);\n");
    output.push_str("    }\n}\n\n");
    Ok(())
}

/// Recognizes the tagged-union shape: two or more `$ref` branches, each an object
/// declaring the same required member with a distinct string `const`.
fn tagged_union_for(schema: &Schema, schemas: &BTreeMap<String, Schema>) -> Option<TaggedUnion> {
    let one_of = schema.one_of.as_ref()?;
    if one_of.len() < 2 || schema.properties.is_some() {
        return None;
    }

    let branch_names = one_of
        .iter()
        .map(|branch| branch.reference.as_deref().map(reference_type_name))
        .collect::<Option<Vec<_>>>()?;

    // The discriminator is a member every branch declares as required with a
    // string `const`.
    let first = schemas.get(branch_names.first()?)?;
    let candidates = first
        .properties
        .as_ref()?
        .iter()
        .filter(|(name, property)| {
            property.const_value.as_ref().is_some_and(Value::is_string)
                && required_fields(first).contains(name.as_str())
        })
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();

    for discriminator in candidates {
        let mut branches = Vec::new();
        for branch_name in &branch_names {
            let Some(branch) = schemas.get(branch_name) else {
                break;
            };
            if !required_fields(branch).contains(discriminator.as_str()) {
                break;
            }
            let Some(tag) = branch
                .properties
                .as_ref()
                .and_then(|properties| properties.get(&discriminator))
                .and_then(|property| property.const_value.as_ref())
                .and_then(Value::as_str)
            else {
                break;
            };
            branches.push((tag.to_string(), branch_name.clone()));
        }
        if branches.len() == branch_names.len() {
            return Some(TaggedUnion {
                discriminator,
                branches,
            });
        }
    }
    None
}

fn render_external_models(json_models: &[PlannedJsonType]) -> Result<RenderedModelFragments> {
    if json_models.is_empty() {
        return Ok(RenderedModelFragments::default());
    }

    let unions = TaggedUnions::resolve(json_models)?;
    let mut output = String::new();
    for (index, model) in json_models.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        if let Some(union) = unions.definition(&model_type_ref(model)) {
            render_tagged_union(&mut output, model, union)?;
        } else {
            render_model(&mut output, model, &unions)?;
        }
    }
    Ok(RenderedModelFragments { body: output })
}

fn render_model(output: &mut String, model: &PlannedJsonType, unions: &TaggedUnions) -> Result<()> {
    let schema = decode_schema(model)?;
    let type_name = model_type_ref(model);
    // A branch of a tagged union derives from that union's base class, so its
    // validator overrides the base's abstract members.
    let base_class = unions.base_of(&type_name);
    render_xml_summary(output, "", schema.description.as_deref());
    if !model_needs_extension_data(&schema)? && !is_open_object(&schema) {
        output.push_str("[JsonUnmappedMemberHandling(JsonUnmappedMemberHandling.Disallow)]\n");
    }
    output.push_str(GENERATED_CODE_ATTRIBUTE);
    output.push('\n');
    output.push_str("public class ");
    output.push_str(&type_name);
    let mut bases = Vec::new();
    if let Some(base_class) = base_class {
        bases.push(base_class.to_string());
    }
    if model_needs_on_deserialized(&schema)? {
        bases.push("IJsonOnDeserialized".to_string());
    }
    if !bases.is_empty() {
        output.push_str(" : ");
        output.push_str(&bases.join(", "));
    }
    output.push_str("\n{\n");

    if typed_map_value_schema(&schema)?.is_none() {
        render_model_constructor(output, model, &schema)?;
        render_model_properties(output, &schema)?;
    }
    render_extension_data_property(output, &schema)?;
    render_constraint_validator(output, &schema, unions.is_branch(&type_name))?;
    render_model_validation(output, &schema)?;

    output.push_str("}\n\n");
    Ok(())
}

/// Emits the constraint validator: a public `Validate()` that aggregates every
/// violation into one [`ValidationException`], plus the `CollectViolations` worker
/// it and any containing model share.
///
/// Two entry points because the contract has to hold in both wire directions.
/// `OnDeserialized` calls `Validate()` so an inbound payload can never enter the
/// process in a shape the contract forbids; `Validate()` is public so the service
/// binding can call it before serializing an outbound value. `CollectViolations`
/// takes a path prefix so a nested model reports `page.blocks.order` rather than
/// a bare `order`.
fn render_constraint_validator(
    output: &mut String,
    schema: &Schema,
    is_union_branch: bool,
) -> Result<()> {
    let constrained = constrained_members(schema);
    let object_constraints = schema.object_constraints()?;
    // A union branch always emits the pair, even with nothing to check, because
    // the base class declares them abstract.
    if !is_union_branch && constrained.is_empty() && object_constraints.is_empty() {
        return Ok(());
    }
    let modifier = if is_union_branch { "override " } else { "" };

    render_pattern_fields(output, &constrained);

    output.push('\n');
    output.push_str("    /// <summary>\n");
    output.push_str(
        "    /// Validates every constraint the contract declares on this type, throwing a\n",
    );
    output.push_str(
        "    /// single <see cref=\"ValidationException\"/> carrying all violations rather\n",
    );
    output.push_str("    /// than stopping at the first.\n");
    output.push_str("    /// </summary>\n");
    output.push_str("    public ");
    output.push_str(modifier);
    output.push_str("void Validate()\n    {\n");
    output.push_str("        var violations = new List<Violation>();\n");
    output.push_str("        CollectViolations(violations, string.Empty);\n");
    output.push_str("        if (violations.Count > 0)\n        {\n");
    output.push_str("            throw new ValidationException(violations);\n");
    output.push_str("        }\n");
    output.push_str("    }\n\n");

    output.push_str("    internal ");
    output.push_str(modifier);
    output.push_str("void CollectViolations(List<Violation> violations, string path)\n    {\n");
    for member in &constrained {
        render_member_constraints(output, member);
    }
    // Object-level checks come after the per-member ones, matching the order Go
    // aggregates them so a multi-violation message reads the same.
    render_object_constraints(output, schema, &object_constraints)?;
    output.push_str("    }\n");
    Ok(())
}

/// Emits the object-level checks: `minProperties`/`maxProperties` over the wire
/// member count, `propertyNames` over each member name, and `dependentRequired`.
fn render_object_constraints(
    output: &mut String,
    schema: &Schema,
    constraints: &ObjectConstraints,
) -> Result<()> {
    if constraints.is_empty() {
        return Ok(());
    }

    if !constraints.count_bounds.is_empty() {
        let count_expr = wire_property_count_expression(schema)?;
        output.push_str("        var propertyCount = ");
        output.push_str(&count_expr);
        output.push_str(";\n");
        for bound in &constraints.count_bounds {
            output.push_str("        if (");
            output.push_str(&bound.violation_condition("propertyCount"));
            output.push_str(")\n        {\n");
            // Object-level violations carry the containing path, with no member
            // segment appended — the failure is the object's, not a member's.
            output.push_str("            violations.Add(new Violation(path, ");
            output.push_str(&csharp_string_literal(&bound.reason_prefix()));
            output.push_str(" + propertyCount));\n");
            output.push_str("        }\n");
        }
    }

    if !constraints.property_name_lengths.is_empty() {
        output.push_str("        foreach (var propertyName in AdditionalProperties.Keys)\n");
        output.push_str("        {\n");
        output.push_str("            var nameLength = JsonRuntime.CodePointCount(propertyName);\n");
        for bound in &constraints.property_name_lengths {
            output.push_str("            if (");
            output.push_str(&bound.violation_condition("nameLength"));
            output.push_str(")\n            {\n");
            output.push_str(
                "                violations.Add(new Violation(JsonRuntime.JoinPath(path, propertyName), ",
            );
            // Interpolated so the reason names the offending key, matching Go's
            // `invalid property name %q: ...`. The path carries the key too, but
            // the duplication is what keeps the diagnostic text identical.
            output.push_str(&format!(
                "$\"invalid property name \\\"{{propertyName}}\\\": {}{{nameLength}}\"",
                bound.reason_prefix()
            ));
            output.push_str("));\n");
            output.push_str("            }\n");
        }
        output.push_str("        }\n");
    }

    for dependency in &constraints.dependent_required {
        let trigger = member_presence_expression(schema, &dependency.trigger);
        let dependent = member_presence_expression(schema, &dependency.dependent);
        output.push_str("        if (");
        output.push_str(&trigger);
        output.push_str(" && !");
        output.push_str(&dependent);
        output.push_str(")\n        {\n");
        output.push_str("            violations.Add(new Violation(JsonRuntime.JoinPath(path, ");
        output.push_str(&csharp_string_literal(&dependency.dependent));
        output.push_str("), ");
        output.push_str(&csharp_string_literal(&format!(
            "property {:?} is required when {:?} is present",
            dependency.dependent, dependency.trigger
        )));
        output.push_str("));\n");
        output.push_str("        }\n");
    }
    Ok(())
}

/// The C# expression for how many members the payload carried.
///
/// A required property is always present, so it contributes a constant; every
/// optional and unknown member lands in the extension bag. Together those cover
/// the whole wire member set exactly.
fn wire_property_count_expression(schema: &Schema) -> Result<String> {
    let required_count = schema
        .properties
        .as_ref()
        .map(|properties| {
            let required = required_fields(schema);
            properties
                .keys()
                .filter(|name| required.contains(name.as_str()))
                .count()
        })
        .unwrap_or(0);
    if !model_needs_extension_data(schema)? {
        // No bag: the member set is exactly the required properties.
        return Ok(required_count.to_string());
    }
    Ok(if required_count == 0 {
        "AdditionalProperties.Count".to_string()
    } else {
        format!("{required_count} + AdditionalProperties.Count")
    })
}

/// The C# expression that is true when `json_name` was present on the wire.
fn member_presence_expression(schema: &Schema, json_name: &str) -> String {
    if required_fields(schema).contains(json_name) {
        // `[JsonRequired]` already guarantees presence.
        return "true".to_string();
    }
    format!(
        "AdditionalProperties.ContainsKey({})",
        csharp_string_literal(json_name)
    )
}

/// A member carrying at least one enforceable constraint, paired with how its
/// value is reached in C#.
struct ConstrainedMember<'a> {
    json_name: &'a str,
    /// The C# property name holding the member's value.
    accessor: String,
    /// True when the member is optional or nullable, so the checks have to be
    /// guarded against the absent case.
    needs_null_guard: bool,
    /// The CLR type the value binds to once unwrapped from its nullable form.
    clr_type: String,
    /// True for a `type: integer` member, which carries the 2^53-1 spec cap
    /// whether or not the schema declares any explicit bound.
    integer_cap: bool,
    enum_membership: Option<EnumMembership>,
    numeric_bounds: Vec<NumericBound<'a>>,
    length_bounds: Vec<LengthBound>,
    item_count_bounds: Vec<ItemCountBound>,
    /// True when `uniqueItems` demands every element be distinct.
    unique_items: bool,
    contains: Option<ContainsCheck>,
    /// The loader-normalized pattern, already end-anchor rewritten for .NET.
    pattern: Option<String>,
}

impl ConstrainedMember<'_> {
    fn has_constraints(&self) -> bool {
        self.integer_cap
            || self.enum_membership.is_some()
            || !self.numeric_bounds.is_empty()
            || !self.length_bounds.is_empty()
            || self.pattern.is_some()
            || !self.item_count_bounds.is_empty()
            || self.unique_items
            || self.contains.is_some()
    }

    /// The private static `Regex` field backing this member's `pattern`. Named
    /// camelCase like the other generated private fields, which also keeps it
    /// from colliding with any PascalCase property.
    fn pattern_field(&self) -> String {
        csharp_parameter_name(&format!("{}-pattern", self.json_name))
    }
}

#[derive(Debug, Clone, Copy)]
struct LengthBound {
    at_least: bool,
    bound: usize,
}

/// A `minItems`/`maxItems` bound over an array's element count.
#[derive(Debug, Clone, Copy)]
struct ItemCountBound {
    at_least: bool,
    bound: usize,
}

impl ItemCountBound {
    fn reason_prefix(&self) -> String {
        let quantifier = if self.at_least { "at least" } else { "at most" };
        format!("must have {quantifier} {} items, got ", self.bound)
    }

    fn violation_condition(&self, count_expr: &str) -> String {
        if self.at_least {
            format!("{count_expr} < {}", self.bound)
        } else {
            format!("{count_expr} > {}", self.bound)
        }
    }
}

/// An `enum` closed value set.
#[derive(Debug)]
struct EnumMembership {
    /// C# literals for each admitted value.
    literals: Vec<String>,
    /// The bracket-list body Go names in the reason, e.g. `"a","b"` or `1,2,3`.
    rendered: String,
    /// Whether the offending value is quoted in the reason (Go's `%q` vs `%v`).
    quotes_value: bool,
}

/// The object-level assertions, checked against the wire member set.
#[derive(Debug, Default)]
struct ObjectConstraints {
    count_bounds: Vec<PropertyCountBound>,
    /// Length bounds applied to every member name (map-shaped objects only).
    property_name_lengths: Vec<LengthBound>,
    dependent_required: Vec<DependentRequired>,
}

impl ObjectConstraints {
    fn is_empty(&self) -> bool {
        self.count_bounds.is_empty()
            && self.property_name_lengths.is_empty()
            && self.dependent_required.is_empty()
    }
}

/// A `minProperties`/`maxProperties` bound over the wire member count.
#[derive(Debug, Clone, Copy)]
struct PropertyCountBound {
    at_least: bool,
    bound: usize,
}

impl PropertyCountBound {
    fn reason_prefix(&self) -> String {
        let quantifier = if self.at_least { "at least" } else { "at most" };
        format!("must have {quantifier} {} properties, got ", self.bound)
    }

    fn violation_condition(&self, count_expr: &str) -> String {
        if self.at_least {
            format!("{count_expr} < {}", self.bound)
        } else {
            format!("{count_expr} > {}", self.bound)
        }
    }
}

/// One `dependentRequired` edge: presence of `trigger` requires `dependent`.
#[derive(Debug)]
struct DependentRequired {
    trigger: String,
    dependent: String,
}

/// A `contains` check over a `const` element, with its `minContains`/`maxContains`
/// occurrence window.
#[derive(Debug)]
struct ContainsCheck {
    /// The C# literal every element is compared against.
    literal: String,
    min: usize,
    max: Option<usize>,
}

impl LengthBound {
    /// The reason wording Go and Java both use, over a **code point** count.
    fn reason_prefix(&self) -> String {
        let comparison = if self.at_least { ">=" } else { "<=" };
        format!("must have length {comparison} {}, got ", self.bound)
    }

    fn violation_condition(&self, length_expr: &str) -> String {
        if self.at_least {
            format!("{length_expr} < {}", self.bound)
        } else {
            format!("{length_expr} > {}", self.bound)
        }
    }
}

fn constrained_members(schema: &Schema) -> Vec<ConstrainedMember<'_>> {
    let required = required_fields(schema);
    let Some(properties) = &schema.properties else {
        return Vec::new();
    };
    properties
        .iter()
        .filter_map(|(json_name, property)| {
            let is_required = required.contains(json_name.as_str());
            let clr_type = constraint_clr_type(property);
            let member = ConstrainedMember {
                json_name,
                accessor: csharp_type_name(json_name),
                needs_null_guard: !is_required || allows_null(property),
                clr_type: clr_type.clone(),
                // Keyed off the resolved CLR type so the emitted comparison can
                // never be against a value of another type. A genuine sum type such
                // as `oneOf: [string, integer]` resolves to its first non-null
                // branch and so is not capped here; Go puts that cap on the
                // integer *branch* type, which arrives with `oneOf` support.
                integer_cap: clr_type == "long" && !property.has_closed_value_set(),
                enum_membership: property.enum_membership(),
                numeric_bounds: property.numeric_bounds(),
                length_bounds: property.length_bounds(),
                pattern: property.dotnet_pattern(),
                item_count_bounds: property.item_count_bounds(),
                unique_items: property.unique_items.unwrap_or(false),
                contains: property.contains_check(),
            };
            member.has_constraints().then_some(member)
        })
        .collect()
}

/// The CLR type a constrained member's value binds to when unwrapped from its
/// nullable form.
fn constraint_clr_type(schema: &Schema) -> String {
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("integer") => "long".to_string(),
        Some("number") => "double".to_string(),
        Some("string") => "string".to_string(),
        // An array binds to the same read-only list shape the property exposes, so
        // the pattern match in the null guard succeeds against the stored value.
        Some("array") => {
            let item = schema
                .items
                .as_deref()
                .map(constraint_clr_type)
                .unwrap_or_else(|| "object".to_string());
            format!("IReadOnlyList<{item}>")
        }
        // Nullable spellings carry the concrete type on the non-null branch.
        _ => schema
            .one_of
            .as_ref()
            .and_then(|branches| {
                branches
                    .iter()
                    .find(|branch| !schema_type_includes(branch, "null"))
                    .map(constraint_clr_type)
            })
            .unwrap_or_else(|| "object".to_string()),
    }
}

/// Emits the `private static readonly Regex` field for every member with a
/// `pattern`, so the expression is compiled once per type rather than per call.
fn render_pattern_fields(output: &mut String, members: &[ConstrainedMember<'_>]) {
    let patterned = members
        .iter()
        .filter(|member| member.pattern.is_some())
        .collect::<Vec<_>>();
    if patterned.is_empty() {
        return;
    }
    output.push('\n');
    for member in patterned {
        let pattern = member.pattern.as_deref().expect("pattern presence checked");
        output.push_str("    private static readonly Regex ");
        output.push_str(&member.pattern_field());
        output.push_str(" = new Regex(");
        output.push_str(&csharp_string_literal(pattern));
        // CultureInvariant so character classes never depend on the ambient
        // locale. Backtracking safety comes from the loader's RE2 gate, which
        // rejects lookaround and backreferences outright.
        output.push_str(", RegexOptions.CultureInvariant);\n");
    }
}

fn render_member_constraints(output: &mut String, member: &ConstrainedMember<'_>) {
    // An optional member arrives as `long?`/`double?`/`string?`; bind it once so
    // every check reads the unwrapped value, and skip them all when it is absent.
    let (indent, value_expr) = if member.needs_null_guard {
        let local = csharp_parameter_name(&format!("{}-value", member.json_name));
        output.push_str("        if (");
        output.push_str(&member.accessor);
        output.push_str(" is ");
        output.push_str(&member.clr_type);
        output.push(' ');
        output.push_str(&local);
        output.push_str(")\n        {\n");
        ("            ", local)
    } else {
        ("        ", member.accessor.clone())
    };

    let add_violation = |output: &mut String, reason_expr: &str| {
        output.push_str(indent);
        output.push_str("    violations.Add(new Violation(JsonRuntime.JoinPath(path, ");
        output.push_str(&csharp_string_literal(member.json_name));
        output.push_str("), ");
        output.push_str(reason_expr);
        output.push_str("));\n");
    };

    if let Some(membership) = &member.enum_membership {
        output.push_str(indent);
        output.push_str("if (");
        for (index, literal) in membership.literals.iter().enumerate() {
            if index > 0 {
                output.push_str(" && ");
            }
            output.push_str(&value_expr);
            output.push_str(" != ");
            output.push_str(literal);
        }
        output.push_str(")\n");
        output.push_str(indent);
        output.push_str("{\n");
        let reason = format!("must be one of [{}], got ", membership.rendered);
        let value_text = if membership.quotes_value {
            // Go uses `%q` for a string value, so the offending value is quoted.
            format!("JsonRuntime.Quote({value_expr})")
        } else {
            format!("JsonRuntime.FormatNumber({value_expr})")
        };
        add_violation(
            output,
            &format!("{} + {value_text}", csharp_string_literal(&reason)),
        );
        output.push_str(indent);
        output.push_str("}\n");
    }

    if member.integer_cap {
        output.push_str(indent);
        output.push_str("if (");
        output.push_str(&value_expr);
        output.push_str(" < -JsonRuntime.IntegerCap || ");
        output.push_str(&value_expr);
        output.push_str(" > JsonRuntime.IntegerCap)\n");
        output.push_str(indent);
        output.push_str("{\n");
        add_violation(
            output,
            &csharp_string_literal("exceeds ±(2^53-1) integer cap"),
        );
        output.push_str(indent);
        output.push_str("}\n");
    }

    for bound in &member.numeric_bounds {
        output.push_str(indent);
        output.push_str("if (");
        output.push_str(&bound.violation_condition(&value_expr));
        output.push_str(")\n");
        output.push_str(indent);
        output.push_str("{\n");
        add_violation(
            output,
            &format!(
                "{} + JsonRuntime.FormatNumber({value_expr})",
                csharp_string_literal(&bound.reason_format())
            ),
        );
        output.push_str(indent);
        output.push_str("}\n");
    }

    // Length is a **code point** count, matching Go's utf8.RuneCountInString and
    // Java's codePointCount. C#'s `string.Length` counts UTF-16 units, which would
    // over-count every astral character.
    if !member.length_bounds.is_empty() {
        let length_local = csharp_parameter_name(&format!("{}-length", member.json_name));
        output.push_str(indent);
        output.push_str("var ");
        output.push_str(&length_local);
        output.push_str(" = JsonRuntime.CodePointCount(");
        output.push_str(&value_expr);
        output.push_str(");\n");
        for bound in &member.length_bounds {
            output.push_str(indent);
            output.push_str("if (");
            output.push_str(&bound.violation_condition(&length_local));
            output.push_str(")\n");
            output.push_str(indent);
            output.push_str("{\n");
            add_violation(
                output,
                &format!(
                    "{} + {length_local}",
                    csharp_string_literal(&bound.reason_prefix())
                ),
            );
            output.push_str(indent);
            output.push_str("}\n");
        }
    }

    if !member.item_count_bounds.is_empty() {
        let count_expr = format!("{value_expr}.Count");
        for bound in &member.item_count_bounds {
            output.push_str(indent);
            output.push_str("if (");
            output.push_str(&bound.violation_condition(&count_expr));
            output.push_str(")\n");
            output.push_str(indent);
            output.push_str("{\n");
            add_violation(
                output,
                &format!(
                    "{} + {count_expr}",
                    csharp_string_literal(&bound.reason_prefix())
                ),
            );
            output.push_str(indent);
            output.push_str("}\n");
        }
    }

    // Reports every duplicate occurrence against the index of its first sighting,
    // matching Go element-for-element rather than stopping at the first pair.
    if member.unique_items {
        output.push_str(indent);
        output.push_str("JsonRuntime.CollectDuplicateItems(");
        output.push_str(&value_expr);
        output.push_str(", JsonRuntime.JoinPath(path, ");
        output.push_str(&csharp_string_literal(member.json_name));
        output.push_str("), violations);\n");
    }

    if let Some(contains) = &member.contains {
        let match_count = csharp_parameter_name(&format!("{}-match-count", member.json_name));
        output.push_str(indent);
        output.push_str("var ");
        output.push_str(&match_count);
        output.push_str(" = JsonRuntime.CountMatchingItems(");
        output.push_str(&value_expr);
        output.push_str(", ");
        output.push_str(&contains.literal);
        output.push_str(");\n");
        output.push_str(indent);
        output.push_str("if (");
        output.push_str(&match_count);
        output.push_str(" < ");
        output.push_str(&contains.min.to_string());
        output.push_str(")\n");
        output.push_str(indent);
        output.push_str("{\n");
        add_violation(
            output,
            &format!(
                "{} + {match_count}",
                csharp_string_literal(&format!(
                    "too few matching items: at least {}, got ",
                    contains.min
                ))
            ),
        );
        output.push_str(indent);
        output.push_str("}\n");
        if let Some(max) = contains.max {
            output.push_str(indent);
            output.push_str("if (");
            output.push_str(&match_count);
            output.push_str(" > ");
            output.push_str(&max.to_string());
            output.push_str(")\n");
            output.push_str(indent);
            output.push_str("{\n");
            add_violation(
                output,
                &format!(
                    "{} + {match_count}",
                    csharp_string_literal(&format!("too many matching items: at most {max}, got "))
                ),
            );
            output.push_str(indent);
            output.push_str("}\n");
        }
    }

    if let Some(pattern) = &member.pattern {
        output.push_str(indent);
        output.push_str("if (!");
        output.push_str(&member.pattern_field());
        output.push_str(".IsMatch(");
        output.push_str(&value_expr);
        output.push_str("))\n");
        output.push_str(indent);
        output.push_str("{\n");
        // Wording follows Java, which like .NET rewrites the `$` end anchor to
        // `\z` and so reports the rewritten pattern. Go quotes via `%q` and keeps
        // `$`, so the two already differ; matching Java is the closest parity
        // available.
        add_violation(
            output,
            &format!(
                "{} + {value_expr}",
                csharp_string_literal(&format!("must match pattern {pattern}, got "))
            ),
        );
        output.push_str(indent);
        output.push_str("}\n");
    }

    if member.needs_null_guard {
        output.push_str("        }\n");
    }
}

fn render_model_constructor(
    output: &mut String,
    model: &PlannedJsonType,
    schema: &Schema,
) -> Result<()> {
    let required = required_fields(schema);
    let required_properties = schema
        .properties
        .as_ref()
        .map(|properties| {
            properties
                .iter()
                .filter(|(name, property)| {
                    required.contains(name.as_str()) && property.const_value.is_none()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if required_properties.is_empty() {
        return Ok(());
    }

    output.push_str("    public ");
    output.push_str(&model_type_ref(model));
    output.push('(');
    for (index, (json_name, property)) in required_properties.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        output.push_str(&schema_type(property, false)?);
        output.push(' ');
        output.push_str(&csharp_parameter_name(json_name));
    }
    output.push_str(")\n    {\n");
    for (json_name, _) in required_properties {
        output.push_str("        ");
        output.push_str(&csharp_type_name(json_name));
        output.push_str(" = ");
        output.push_str(&csharp_parameter_name(json_name));
        output.push_str(";\n");
    }
    output.push_str("    }\n\n");
    Ok(())
}

fn render_model_properties(output: &mut String, schema: &Schema) -> Result<()> {
    let required = required_fields(schema);
    let Some(properties) = &schema.properties else {
        return Ok(());
    };
    for (json_name, property) in properties {
        render_xml_summary(output, "    ", property.description.as_deref());
        if !required.contains(json_name.as_str()) {
            render_optional_property(output, json_name, property)?;
            continue;
        }
        if property.const_value.is_some() {
            render_const_property(output, json_name, property)?;
            continue;
        }
        output.push_str("    [JsonPropertyName(");
        output.push_str(&csharp_string_literal(json_name));
        output.push_str(")]\n");
        output.push_str("    [JsonRequired]\n");
        output.push_str("    public ");
        output.push_str(&schema_type(property, false)?);
        output.push(' ');
        output.push_str(&csharp_type_name(json_name));
        output.push_str(" { get; init; }\n");
    }
    Ok(())
}

fn render_optional_property(output: &mut String, json_name: &str, property: &Schema) -> Result<()> {
    let property_type = schema_type(property, true)?;
    output.push_str("    [JsonIgnore]\n");
    output.push_str("    public ");
    output.push_str(&property_type);
    output.push(' ');
    output.push_str(&csharp_type_name(json_name));
    output.push_str("\n    {\n");
    output.push_str("        get => JsonRuntime.ReadOptionalValue<");
    output.push_str(&optional_read_type(property, &property_type)?);
    output.push_str(">(AdditionalProperties, ");
    output.push_str(&csharp_string_literal(json_name));
    if let Some(default_value) = property.default.as_ref().and_then(csharp_value_literal) {
        output.push_str(", ");
        output.push_str(&default_value);
    }
    output.push_str(");\n");
    output.push_str("        init\n        {\n");
    if !allows_null(property) {
        output.push_str("            JsonRuntime.RejectNull(");
        output.push_str(&csharp_string_literal(json_name));
        output.push_str(", value);\n");
    }
    output.push_str("            AdditionalProperties[");
    output.push_str(&csharp_string_literal(json_name));
    output.push_str("] = value;\n");
    output.push_str("        }\n");
    output.push_str("    }\n");
    Ok(())
}

fn render_const_property(output: &mut String, json_name: &str, property: &Schema) -> Result<()> {
    let const_value = property
        .const_value
        .as_ref()
        .and_then(csharp_value_literal)
        .expect("const property should have C# literal");
    let field_name = csharp_parameter_name(&format!("{json_name}-value"));
    output.push_str("    private ");
    output.push_str(&schema_type(property, false)?);
    output.push(' ');
    output.push_str(&field_name);
    output.push_str(" = ");
    output.push_str(&const_value);
    output.push_str(";\n\n");
    output.push_str("    [JsonPropertyName(");
    output.push_str(&csharp_string_literal(json_name));
    output.push_str(")]\n");
    output.push_str("    [JsonRequired]\n");
    output.push_str("    public ");
    output.push_str(&schema_type(property, false)?);
    output.push(' ');
    output.push_str(&csharp_type_name(json_name));
    output.push_str("\n    {\n");
    output.push_str("        get => ");
    output.push_str(&field_name);
    output.push_str(";\n");
    output.push_str("        init\n        {\n");
    output.push_str("            if (value != ");
    output.push_str(&const_value);
    output.push_str(")\n            {\n");
    output.push_str("                throw new JsonException(");
    output.push_str(&csharp_string_literal(&format!(
        "{json_name} must equal {const_value}"
    )));
    output.push_str(");\n");
    output.push_str("            }\n");
    output.push_str("            ");
    output.push_str(&field_name);
    output.push_str(" = value;\n");
    output.push_str("        }\n");
    output.push_str("    }\n");
    Ok(())
}

fn optional_read_type(schema: &Schema, property_type: &str) -> Result<String> {
    if matches!(schema.ty.as_ref().and_then(Value::as_str), Some("array")) {
        let item = schema
            .items
            .as_ref()
            .map(|item| schema_base_type(item))
            .transpose()?
            .unwrap_or_else(|| "object".to_string());
        Ok(format!("List<{item}>?"))
    } else {
        Ok(property_type.to_string())
    }
}

fn render_extension_data_property(output: &mut String, schema: &Schema) -> Result<()> {
    if !model_needs_extension_data(schema)? {
        return Ok(());
    }
    if schema
        .properties
        .as_ref()
        .is_some_and(|properties| !properties.is_empty())
    {
        output.push('\n');
    }
    output.push_str("    [JsonExtensionData]\n");
    output.push_str("    public Dictionary<string, object?> AdditionalProperties { get; set; } = new Dictionary<string, object?>();\n");
    Ok(())
}

fn render_model_validation(output: &mut String, schema: &Schema) -> Result<()> {
    if !model_needs_on_deserialized(schema)? {
        return Ok(());
    }
    output.push('\n');
    output.push_str("    void IJsonOnDeserialized.OnDeserialized()\n    {\n");
    if let Some(value_schema) = typed_map_value_schema(schema)? {
        output.push_str("        foreach (var entry in AdditionalProperties)\n        {\n");
        render_extension_value_validation(output, "entry.Key", "entry.Value", &value_schema, 3)?;
        output.push_str("        }\n");
        if !schema.object_constraints()?.is_empty() {
            output.push_str("        Validate();\n");
        }
        output.push_str("    }\n");
        return Ok(());
    }

    let optional_fields = optional_fields(schema);
    if !optional_fields.is_empty() {
        if !is_open_object(schema) {
            output.push_str("        foreach (var key in AdditionalProperties.Keys)\n        {\n");
            output.push_str("            if (");
            for (index, (json_name, _)) in optional_fields.iter().enumerate() {
                if index > 0 {
                    output.push_str(" && ");
                }
                output.push_str("key != ");
                output.push_str(&csharp_string_literal(json_name));
            }
            output.push_str(")\n            {\n");
            output.push_str(
                "                throw new JsonException($\"Unknown field `{key}`.\");\n",
            );
            output.push_str("            }\n");
            output.push_str("        }\n");
        }
        for (json_name, property) in optional_fields {
            let value_name = csharp_parameter_name(&format!("{json_name}-value"));
            output.push_str("        if (AdditionalProperties.TryGetValue(");
            output.push_str(&csharp_string_literal(json_name));
            output.push_str(", out var ");
            output.push_str(&value_name);
            output.push_str("))\n        {\n");
            if !allows_null(property) {
                output.push_str("            JsonRuntime.RejectNull(");
                output.push_str(&csharp_string_literal(json_name));
                output.push_str(", ");
                output.push_str(&value_name);
                output.push_str(");\n");
            }
            render_extension_value_validation(
                output,
                &csharp_string_literal(json_name),
                &value_name,
                property,
                3,
            )?;
            output.push_str("        }\n");
        }
    }
    // Structural checks above reject a malformed payload outright; the contract
    // constraints then run so an inbound value cannot enter the process in a shape
    // the contract forbids.
    if !constrained_members(schema).is_empty() || !schema.object_constraints()?.is_empty() {
        output.push_str("        Validate();\n");
    }
    output.push_str("    }\n");
    Ok(())
}

fn render_extension_value_validation(
    output: &mut String,
    path_expr: &str,
    value_expr: &str,
    schema: &Schema,
    indent_level: usize,
) -> Result<()> {
    if allows_null(schema) {
        return Ok(());
    }
    let indent = "    ".repeat(indent_level);
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => {
            let json_name = format!("json{indent_level}");
            output.push_str(&indent);
            output.push_str("if (");
            output.push_str(value_expr);
            output.push_str(" is JsonElement ");
            output.push_str(&json_name);
            output.push_str(" && ");
            output.push_str(&json_name);
            output.push_str(".ValueKind != JsonValueKind.String)\n");
            output.push_str(&indent);
            output.push_str("{\n");
            output.push_str(&indent);
            output.push_str("    throw new JsonException($\"{");
            output.push_str(path_expr);
            output.push_str("}: expected string\");\n");
            output.push_str(&indent);
            output.push_str("}\n");
            output.push_str(&indent);
            output.push_str("else if (");
            output.push_str(value_expr);
            output.push_str(" is not JsonElement && ");
            output.push_str(value_expr);
            output.push_str(" is not string)\n");
            output.push_str(&indent);
            output.push_str("{\n");
            output.push_str(&indent);
            output.push_str("    throw new JsonException($\"{");
            output.push_str(path_expr);
            output.push_str("}: expected string\");\n");
            output.push_str(&indent);
            output.push_str("}\n");
        }
        Some("integer") => {
            output.push_str(&indent);
            output.push_str("_ = JsonRuntime.ReadJsonValue<long?>(");
            output.push_str(value_expr);
            output.push_str(");\n");
        }
        Some("array") => {
            output.push_str(&indent);
            output.push_str("_ = JsonRuntime.ReadJsonValue<");
            output.push_str(&optional_read_type(schema, &schema_type(schema, true)?)?);
            output.push_str(">(");
            output.push_str(value_expr);
            output.push_str(");\n");
        }
        _ if schema.reference.is_some() => {
            output.push_str(&indent);
            output.push_str("_ = JsonRuntime.ReadJsonValue<");
            output.push_str(&optional_read_type(schema, &schema_type(schema, true)?)?);
            output.push_str(">(");
            output.push_str(value_expr);
            output.push_str(");\n");
        }
        _ => {}
    }
    Ok(())
}

fn decode_schema(model: &PlannedJsonType) -> Result<Schema> {
    serde_json::from_value(model.schema.clone()).map_err(|error| Error::InvalidJsonSchema {
        path: PathBuf::from("<json-generator>"),
        reason: format!(
            "failed to read planned JSON schema `{}`: {error}",
            model.full_name
        ),
    })
}

fn typed_map_value_schema(schema: &Schema) -> Result<Option<Schema>> {
    if schema
        .properties
        .as_ref()
        .is_some_and(|properties| !properties.is_empty())
    {
        return Ok(None);
    }

    match &schema.additional_properties {
        Some(Value::Object(_)) => serde_json::from_value(
            schema
                .additional_properties
                .clone()
                .expect("additional properties presence checked"),
        )
        .map(Some)
        .map_err(|error| Error::InvalidJsonSchema {
            path: PathBuf::from("<json-generator>"),
            reason: format!("failed to read `additionalProperties`: {error}"),
        }),
        _ => Ok(None),
    }
}

fn model_needs_extension_data(schema: &Schema) -> Result<bool> {
    Ok(typed_map_value_schema(schema)?.is_some()
        || is_open_object(schema)
        || !optional_fields(schema).is_empty())
}

fn model_needs_on_deserialized(schema: &Schema) -> Result<bool> {
    // A model whose only validation is constraint checking still needs the hook,
    // so an inbound payload is validated on deserialize.
    Ok(!constrained_members(schema).is_empty()
        || !schema.object_constraints()?.is_empty()
        || typed_map_value_schema(schema)?.is_some()
        || (!optional_fields(schema).is_empty()
            && (!is_open_object(schema)
                || optional_fields(schema)
                    .iter()
                    .any(|(_, property)| !allows_null(property)))))
}

fn optional_fields(schema: &Schema) -> Vec<(&str, &Schema)> {
    let required = required_fields(schema);
    schema
        .properties
        .as_ref()
        .map(|properties| {
            properties
                .iter()
                .filter(|(name, _)| !required.contains(name.as_str()))
                .map(|(name, property)| (name.as_str(), property))
                .collect()
        })
        .unwrap_or_default()
}

fn schema_type(schema: &Schema, optional: bool) -> Result<String> {
    let base = schema_base_type(schema)?;
    Ok(if optional { nullable_type(&base) } else { base })
}

fn schema_base_type(schema: &Schema) -> Result<String> {
    if let Some(reference) = &schema.reference {
        return Ok(reference_type_name(reference));
    }
    if let Some(one_of) = &schema.one_of {
        let non_null = one_of
            .iter()
            .filter(|branch| !schema_type_includes(branch, "null"))
            .collect::<Vec<_>>();
        if one_of
            .iter()
            .any(|branch| schema_type_includes(branch, "null"))
            && let Some(branch) = non_null.first()
        {
            return Ok(nullable_type(&schema_base_type(branch)?));
        }
        return Ok("object".to_string());
    }
    match schema.ty.as_ref().and_then(Value::as_str) {
        Some("string") => Ok("string".to_string()),
        Some("integer") => Ok("long".to_string()),
        Some("number") => Ok("double".to_string()),
        Some("boolean") => Ok("bool".to_string()),
        Some("array") => {
            let item = schema
                .items
                .as_ref()
                .map(|item| schema_base_type(item))
                .transpose()?
                .unwrap_or_else(|| "object".to_string());
            Ok(format!("IReadOnlyList<{item}>"))
        }
        Some("object") => {
            if let Some(value_schema) = typed_map_value_schema(schema)? {
                Ok(format!(
                    "IReadOnlyDictionary<string, {}>",
                    schema_base_type(&value_schema)?
                ))
            } else {
                Ok("object".to_string())
            }
        }
        Some("null") => Ok("object?".to_string()),
        _ => Ok("object".to_string()),
    }
}

fn nullable_type(base: &str) -> String {
    if base.ends_with('?') {
        base.to_string()
    } else {
        format!("{base}?")
    }
}

fn required_fields(schema: &Schema) -> BTreeSet<&str> {
    schema
        .required
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect()
}

fn is_open_object(schema: &Schema) -> bool {
    !matches!(schema.additional_properties, Some(Value::Bool(false)))
}

fn allows_null(schema: &Schema) -> bool {
    schema_type_includes(schema, "null")
        || schema.one_of.as_ref().is_some_and(|branches| {
            branches
                .iter()
                .any(|branch| schema_type_includes(branch, "null"))
        })
}

fn schema_type_includes(schema: &Schema, ty: &str) -> bool {
    match &schema.ty {
        Some(Value::String(value)) => value == ty,
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|value| value == ty)),
        _ => false,
    }
}

fn reference_model_name(reference: &str) -> String {
    reference
        .rsplit('/')
        .next()
        .unwrap_or(reference)
        .rsplit('#')
        .next()
        .unwrap_or(reference)
        .replace("~1", "/")
        .replace("~0", "~")
}

fn reference_type_name(reference: &str) -> String {
    let target = reference
        .split_once('#')
        .map(|(_, fragment)| fragment)
        .unwrap_or(reference)
        .strip_prefix("/$defs/")
        .map(|name| name.replace("~1", "/").replace("~0", "~"));
    if let Some(target) = target
        && let Some((module_key, model_name)) = target.rsplit_once('#')
        && !module_key.is_empty()
    {
        let namespace = module_key
            .split('/')
            .map(|segment| csharp_type_name(&segment.to_upper_camel_case()))
            .collect::<Vec<_>>()
            .join(".");
        return format!(
            "global::NexGen.Generated.{}.{}",
            namespace,
            csharp_type_name(model_name)
        );
    }
    csharp_type_name(&reference_model_name(reference))
}

fn csharp_value_literal(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(csharp_string_literal(value)),
        Value::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Null => Some("null".to_string()),
        _ => None,
    }
}

fn render_xml_summary(output: &mut String, indent: &str, summary: Option<&str>) {
    let Some(summary) = summary else {
        return;
    };
    output.push_str(indent);
    output.push_str("/// <summary>\n");
    for line in summary.trim().lines() {
        output.push_str(indent);
        output.push_str("/// ");
        output.push_str(&xml_doc_escape(line.trim()));
        output.push('\n');
    }
    output.push_str(indent);
    output.push_str("/// </summary>\n");
}

fn xml_doc_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
