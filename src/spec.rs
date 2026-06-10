use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use heck::{ToSnakeCase, ToUpperCamelCase};
use tempfile::TempDir;
use wit_parser::{
    Function, FunctionKind, Handle, Interface, PackageId, PackageSourceMap, Resolve, Type, TypeDef,
    TypeDefKind, TypeId, TypeOwner, WorldItem, WorldKey,
};

use crate::error::{Error, Result};
use crate::language::Language;

type PackageOrigins = BTreeMap<PackageId, PathBuf>;

pub(crate) struct ParsedWitPackage {
    pub resolve: Resolve,
    pub package_id: PackageId,
    pub package_origins: PackageOrigins,
    _workspace: TempDir,
}

fn split_input_paths(input_paths: &[PathBuf]) -> Result<(&PathBuf, &[PathBuf])> {
    input_paths.split_first().ok_or_else(|| Error::InvalidWit {
        path: PathBuf::from("<input>"),
        reason: "at least one WIT input path is required".to_string(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiSpec {
    pub version: String,
    pub support: SupportSpec,
    pub language_imports: BTreeMap<Language, Vec<LanguageImportSpec>>,
    pub services: Vec<ServiceSpec>,
    pub types: BTreeMap<String, TypeOverrideSpec>,
    pub records: BTreeMap<String, WitRecordSpec>,
    pub enums: BTreeMap<String, WitEnumSpec>,
    pub flags: BTreeMap<String, WitFlagsSpec>,
    pub variants: BTreeMap<String, WitVariantSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct LanguageImportSpec {
    pub language: Language,
    pub reference: String,
    pub module: String,
    pub name: Option<String>,
    pub type_only: bool,
    pub import_style: LanguageImportStyle,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum LanguageImportStyle {
    Module,
    Namespace,
    Named,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedTypeMetadata {
    pub wit_name: String,
    pub use_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedWitMetadata {
    pub proto_types: BTreeMap<String, LinkedTypeMetadata>,
    pub type_compatibility: BTreeMap<String, BTreeSet<String>>,
    pub type_covered_fields: BTreeMap<String, BTreeSet<String>>,
    pub type_use_paths: BTreeMap<String, String>,
}

impl ApiSpec {
    pub fn load_for_language(language: Language, path: &Path) -> Result<Self> {
        let input = fs::read_to_string(path).map_err(|source| Error::ReadFile {
            path: path.to_path_buf(),
            source,
        })?;
        Self::parse_for_language(language, &input, path.to_path_buf())
    }

    pub fn load_for_language_with_inputs(
        language: Language,
        input_paths: &[PathBuf],
    ) -> Result<Self> {
        let (path, linked_input_paths) = split_input_paths(input_paths)?;
        let input = fs::read_to_string(path).map_err(|source| Error::ReadFile {
            path: path.clone(),
            source,
        })?;
        Self::parse_for_language_with_inputs(language, &input, path.clone(), linked_input_paths)
    }

    pub fn parse_for_language(language: Language, input: &str, path: PathBuf) -> Result<Self> {
        Self::parse_for_language_with_inputs(language, input, path, &[])
    }

    pub fn parse_for_language_with_inputs(
        language: Language,
        input: &str,
        path: PathBuf,
        linked_input_paths: &[PathBuf],
    ) -> Result<Self> {
        Self::parse_with_inputs(input, path, language, linked_input_paths)
    }

    pub fn parse(input: &str, path: PathBuf, language: Language) -> Result<Self> {
        Self::parse_with_inputs(input, path, language, &[])
    }

    pub fn parse_with_inputs(
        input: &str,
        path: PathBuf,
        language: Language,
        linked_input_paths: &[PathBuf],
    ) -> Result<Self> {
        let parsed = parse_wit_with_inputs(input, &path, linked_input_paths)?;
        Self::from_wit(
            &parsed.resolve,
            parsed.package_id,
            &parsed.package_origins,
            path,
            language,
        )
    }

    pub fn type_override(&self, type_name: &str) -> Option<&TypeOverrideSpec> {
        self.types.get(type_name.trim_start_matches('.'))
    }

    fn from_wit(
        resolve: &Resolve,
        package_id: PackageId,
        package_origins: &PackageOrigins,
        path: PathBuf,
        language: Language,
    ) -> Result<Self> {
        let package = &resolve.packages[package_id];
        let world_id = select_world(resolve, package_id, &path)?;
        let world = &resolve.worlds[world_id];
        let support = collect_support_spec(resolve, package_id, package_origins)?;
        let language_imports = collect_language_imports(resolve, package_origins, &path)?;

        let mut types = BTreeMap::new();
        let mut records = BTreeMap::new();
        let mut enums = BTreeMap::new();
        let mut flags = BTreeMap::new();
        let mut variants = BTreeMap::new();
        for (_, dependency_package) in resolve.packages.iter() {
            for interface_id in dependency_package.interfaces.values() {
                let interface = &resolve.interfaces[*interface_id];
                collect_interface_types(
                    resolve,
                    interface,
                    &path,
                    language,
                    &mut types,
                    &mut records,
                    &mut enums,
                    &mut flags,
                    &mut variants,
                )?;
            }
        }

        let mut services = Vec::new();
        for (key, item) in &world.exports {
            let WorldItem::Interface { id, .. } = item else {
                continue;
            };
            let interface = &resolve.interfaces[*id];
            services.push(build_service(resolve, key, interface, &path, language)?);
        }

        Ok(Self {
            version: package
                .name
                .version
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "0.0.0".to_string()),
            support,
            language_imports,
            services,
            types,
            records,
            enums,
            flags,
            variants,
        })
    }

    pub fn imports_for_language(&self, language: Language) -> &[LanguageImportSpec] {
        self.language_imports
            .get(&language)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

pub fn write_prepared_wit_directory(input_paths: &[PathBuf], output_path: &Path) -> Result<()> {
    if output_path.exists() {
        return Err(Error::OutputPathExists {
            path: output_path.to_path_buf(),
        });
    }

    let (input_path, linked_input_paths) = split_input_paths(input_paths)?;
    let input = fs::read_to_string(input_path).map_err(|source| Error::ReadFile {
        path: input_path.clone(),
        source,
    })?;
    let workspace = prepare_wit_workspace(&input, input_path, linked_input_paths)?;
    copy_directory_tree(&workspace.package_root, output_path)?;
    Ok(())
}

pub(crate) fn parse_wit_with_inputs(
    input: &str,
    path: &Path,
    linked_input_paths: &[PathBuf],
) -> Result<ParsedWitPackage> {
    let workspace = prepare_wit_workspace(input, path, linked_input_paths)?;
    parse_prepared_wit_workspace(workspace, path)
}

fn parse_prepared_wit_workspace(
    workspace: PreparedWitWorkspace,
    path: &Path,
) -> Result<ParsedWitPackage> {
    let mut resolve = Resolve::default();
    let (package_id, source_map) =
        resolve
            .push_dir(&workspace.package_root)
            .map_err(|error| Error::WitParse {
                path: path.to_path_buf(),
                message: format_error_chain(&error),
            })?;
    let package_origins = collect_package_origins(&resolve, &source_map)?;
    Ok(ParsedWitPackage {
        resolve,
        package_id,
        package_origins,
        _workspace: workspace.temp_dir,
    })
}

fn format_error_chain(error: &impl std::fmt::Display) -> String {
    format!("{error:#}")
}

pub fn load_linked_wit_metadata_from_inputs(input_paths: &[PathBuf]) -> Result<LinkedWitMetadata> {
    let workspace = prepare_linked_metadata_workspace(input_paths)?;
    let mut resolve = Resolve::default();
    let (main_package_id, source_map) =
        resolve
            .push_dir(&workspace.package_root)
            .map_err(|error| Error::InvalidWit {
                path: input_paths
                    .first()
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from("<input>")),
                reason: format!("failed to parse Temporal type WIT input: {error}"),
            })?;
    let package_origins = collect_package_origins(&resolve, &source_map)?;

    let mut proto_types = BTreeMap::new();
    let mut type_compatibility = BTreeMap::<String, BTreeSet<String>>::new();
    let mut type_covered_fields = BTreeMap::<String, BTreeSet<String>>::new();
    let mut type_use_paths = BTreeMap::new();

    for (package_id, package) in resolve.packages.iter() {
        if package_id == main_package_id {
            continue;
        }

        let package_name = if let Some(version) = &package.name.version {
            format!(
                "{}:{}@{}",
                package.name.namespace, package.name.name, version
            )
        } else {
            format!("{}:{}", package.name.namespace, package.name.name)
        };
        let origin_path = package_origins
            .get(&package_id)
            .cloned()
            .or_else(|| input_paths.first().cloned())
            .unwrap_or_else(|| PathBuf::from("<input>"));

        for interface_id in package.interfaces.values() {
            let interface = &resolve.interfaces[*interface_id];
            let Some(interface_name) = interface.name.as_deref() else {
                continue;
            };
            let use_path = if let Some(version) = &package.name.version {
                format!(
                    "{}:{}/{}@{}",
                    package.name.namespace, package.name.name, interface_name, version
                )
            } else {
                format!(
                    "{}:{}/{}",
                    package.name.namespace, package.name.name, interface_name
                )
            };

            for type_id in interface.types.values() {
                let type_def = &resolve.types[*type_id];
                let Some(type_name) = type_def.name.as_deref() else {
                    continue;
                };
                let context =
                    format!("linked WIT type `{package_name}.{interface_name}.{type_name}`");
                let directives =
                    parse_directives(type_def.docs.contents.as_deref(), &origin_path, &context)?;

                for directive in &directives {
                    if directive.name != "add-rpc-compatible-with" {
                        continue;
                    }
                    let Some(target) = directive.value("value") else {
                        return Err(Error::InvalidWitDirective {
                            path: origin_path.join("model.wit"),
                            context: context.clone(),
                            directive: "@nexus.add-rpc-compatible-with".to_string(),
                            reason: "missing compatibility target".to_string(),
                        });
                    };
                    type_compatibility
                        .entry(type_name.to_string())
                        .or_default()
                        .insert(target.to_string());
                }

                for directive in &directives {
                    if directive.name != "function" && directive.name != "typescript-with-arguments"
                    {
                        continue;
                    }
                    let Some(signature_name) = directive.value("signature") else {
                        continue;
                    };
                    let covered_field = if let Some(args_field) = directive.value("args-field") {
                        args_field.to_string()
                    } else {
                        resolve_function_signature_args(
                            &resolve,
                            type_def,
                            signature_name,
                            &origin_path,
                            &context,
                        )?
                        .0
                    }
                    .replace('-', "_");
                    type_covered_fields
                        .entry(type_name.to_string())
                        .or_default()
                        .insert(covered_field);
                }

                if let Some(existing) =
                    type_use_paths.insert(type_name.to_string(), use_path.clone())
                {
                    if existing != use_path {
                        return Err(Error::InvalidWit {
                            path: origin_path.join("model.wit"),
                            reason: format!(
                                "linked WIT type `{type_name}` is declared under multiple use paths"
                            ),
                        });
                    }
                }

                let Some(proto_name) =
                    directive_value(&directives, "proto", &origin_path, &context, "value")?
                else {
                    continue;
                };

                if let Some(existing) = proto_types.insert(
                    proto_name.clone(),
                    LinkedTypeMetadata {
                        wit_name: type_name.to_string(),
                        use_path: use_path.clone(),
                    },
                ) {
                    return Err(Error::InvalidWit {
                        path: origin_path.join("model.wit"),
                        reason: format!(
                            "duplicate linked `@nexus.proto` mapping for `{proto_name}` (`{}` and `{}`)",
                            existing.wit_name, type_name
                        ),
                    });
                }
            }
        }
    }

    Ok(LinkedWitMetadata {
        proto_types,
        type_compatibility,
        type_covered_fields,
        type_use_paths,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceSpec {
    pub name: String,
    pub wire_name: String,
    pub endpoint: Option<String>,
    pub experimental: bool,
    pub delay_load_temporalio_workflow: bool,
    pub operations: Vec<OperationSpec>,
    pub resources: Vec<ResourceSpec>,
}

impl ServiceSpec {
    pub fn operation(&self, name: &str) -> Option<&OperationSpec> {
        self.operations
            .iter()
            .find(|operation| operation.name == name)
    }

    pub fn resource(&self, name: &str) -> Option<&ResourceSpec> {
        self.resources.iter().find(|resource| resource.name == name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SupportSpec {
    pub fragments: BTreeMap<Language, Vec<SupportFragmentSpec>>,
}

impl SupportSpec {
    pub fn fragments_for_language(&self, language: Language) -> &[SupportFragmentSpec] {
        self.fragments
            .get(&language)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportFragmentSpec {
    pub path: String,
    pub contents: String,
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OperationSpec {
    pub name: String,
    pub wire_name: String,
    pub experimental: bool,
    pub doc: LanguageStringSpec,
    pub return_doc: LanguageStringSpec,
    pub input_proto: String,
    pub output_proto: String,
    pub input_record: Option<String>,
    pub output_record: Option<String>,
    pub output_resource: Option<String>,
    pub output_transform: Option<OperationOutputTransformSpec>,
}

impl OperationSpec {
    pub fn input_proto(&self) -> Option<&str> {
        (!self.input_proto.is_empty()).then_some(self.input_proto.as_str())
    }

    pub fn output_proto(&self) -> Option<&str> {
        (!self.output_proto.is_empty()).then_some(self.output_proto.as_str())
    }

    pub fn input_record(&self) -> Option<&str> {
        self.input_record.as_deref()
    }

    pub fn output_record(&self) -> Option<&str> {
        self.output_record.as_deref()
    }

    pub fn output_resource(&self) -> Option<&str> {
        self.output_resource.as_deref()
    }

    pub fn output_transform(&self) -> Option<&OperationOutputTransformSpec> {
        self.output_transform.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpec {
    pub name: String,
    pub fields: Vec<ResourceFieldSpec>,
    pub methods: Vec<ResourceMethodSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceFieldSpec {
    pub name: String,
    pub optional: bool,
    pub field_type: AuthoredFieldTypeSpec,
    pub function: Option<FunctionFieldSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceMethodSpec {
    pub name: String,
    pub params: Vec<ResourceFieldSpec>,
    pub result: Option<ResourceResultSpec>,
    pub operation_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceResultSpec {
    pub result_type: AuthoredFieldTypeSpec,
    pub proto: Option<String>,
    pub resource: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitRecordSpec {
    pub name: String,
    pub full_name: String,
    pub experimental: bool,
    pub required_fields: BTreeSet<String>,
    pub generated_model: GeneratedModelSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitEnumSpec {
    pub name: String,
    pub full_name: String,
    pub values: Vec<WitEnumValueSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitEnumValueSpec {
    pub wit_name: String,
    pub name: String,
    pub number: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitFlagsSpec {
    pub name: String,
    pub full_name: String,
    pub flags: Vec<WitFlagSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitFlagSpec {
    pub name: String,
    pub bit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitVariantSpec {
    pub name: String,
    pub full_name: String,
    pub cases: Vec<WitVariantCaseSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WitVariantCaseSpec {
    pub name: String,
    pub payload: Option<AuthoredFieldTypeSpec>,
}

fn collect_support_spec(
    resolve: &Resolve,
    current_package_id: PackageId,
    package_origins: &PackageOrigins,
) -> Result<SupportSpec> {
    let mut fragments = BTreeMap::new();

    for language in all_languages() {
        let mut language_fragments = Vec::new();
        let mut seen_paths = BTreeSet::new();

        for (package_id, origin_path) in package_origins {
            if *package_id == current_package_id {
                continue;
            }
            collect_package_support_fragments(
                language,
                resolve,
                *package_id,
                origin_path,
                &mut seen_paths,
                &mut language_fragments,
            )?;
        }

        if let Some(origin_path) = package_origins.get(&current_package_id) {
            collect_package_support_fragments(
                language,
                resolve,
                current_package_id,
                origin_path,
                &mut seen_paths,
                &mut language_fragments,
            )?;
        }

        if !language_fragments.is_empty() {
            fragments.insert(language, language_fragments);
        }
    }

    Ok(SupportSpec { fragments })
}

fn collect_package_support_fragments(
    language: Language,
    resolve: &Resolve,
    package_id: PackageId,
    origin_path: &Path,
    seen_paths: &mut BTreeSet<String>,
    fragments: &mut Vec<SupportFragmentSpec>,
) -> Result<()> {
    let package = &resolve.packages[package_id];
    let package_name = if let Some(version) = &package.name.version {
        format!(
            "{}:{}@{}",
            package.name.namespace, package.name.name, version
        )
    } else {
        format!("{}:{}", package.name.namespace, package.name.name)
    };

    collect_support_fragment_from_docs(
        language,
        package.docs.contents.as_deref(),
        origin_path,
        &format!("package `{package_name}`"),
        seen_paths,
        fragments,
    )?;

    for (world_name, world_id) in &package.worlds {
        let world = &resolve.worlds[*world_id];
        collect_support_fragment_from_docs(
            language,
            world.docs.contents.as_deref(),
            origin_path,
            &format!("package `{package_name}` world `{world_name}`"),
            seen_paths,
            fragments,
        )?;
    }

    Ok(())
}

fn collect_support_fragment_from_docs(
    language: Language,
    docs: Option<&str>,
    origin_path: &Path,
    context: &str,
    seen_paths: &mut BTreeSet<String>,
    fragments: &mut Vec<SupportFragmentSpec>,
) -> Result<()> {
    let directives = parse_directives(docs, origin_path, context)?;
    let Some(relative_path) =
        directive_value_for_language(&directives, "support", origin_path, context, language)?
    else {
        return Ok(());
    };
    let prefix = directive_value_for_language(
        &directives,
        "support-prefix",
        origin_path,
        context,
        language,
    )?;

    let resolved_path = resolve_support_path(origin_path, &relative_path);
    let normalized_path = resolved_path.to_string_lossy().replace('\\', "/");
    if !seen_paths.insert(normalized_path.clone()) {
        return Ok(());
    }

    let contents = load_support_fragment_contents(&resolved_path)?;
    fragments.push(SupportFragmentSpec {
        path: normalized_path,
        contents,
        prefix,
    });
    Ok(())
}

fn load_support_fragment_contents(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|source| Error::ReadFile {
        path: path.to_path_buf(),
        source,
    })
}

fn resolve_support_path(base_dir: &Path, support_path: &str) -> PathBuf {
    let support_path = PathBuf::from(support_path);
    if support_path.is_absolute() {
        support_path
    } else {
        base_dir.join(support_path)
    }
}

fn collect_language_imports(
    resolve: &Resolve,
    package_origins: &PackageOrigins,
    fallback_path: &Path,
) -> Result<BTreeMap<Language, Vec<LanguageImportSpec>>> {
    let mut imports = BTreeSet::new();
    for (package_id, package) in resolve.packages.iter() {
        let origin_path = package_origins
            .get(&package_id)
            .map(PathBuf::as_path)
            .unwrap_or(fallback_path);
        let package_name = if let Some(version) = &package.name.version {
            format!(
                "{}:{}@{}",
                package.name.namespace, package.name.name, version
            )
        } else {
            format!("{}:{}", package.name.namespace, package.name.name)
        };

        collect_language_imports_from_docs(
            package.docs.contents.as_deref(),
            origin_path,
            &format!("package `{package_name}`"),
            &mut imports,
        )?;

        for (world_name, world_id) in &package.worlds {
            let world = &resolve.worlds[*world_id];
            collect_language_imports_from_docs(
                world.docs.contents.as_deref(),
                origin_path,
                &format!("package `{package_name}` world `{world_name}`"),
                &mut imports,
            )?;
        }

        for (interface_name, interface_id) in &package.interfaces {
            let interface = &resolve.interfaces[*interface_id];
            collect_language_imports_from_docs(
                interface.docs.contents.as_deref(),
                origin_path,
                &format!("interface `{interface_name}`"),
                &mut imports,
            )?;

            for type_id in interface.types.values() {
                let type_def = &resolve.types[*type_id];
                let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
                let type_context = format!("type `{interface_name}.{type_name}`");
                collect_language_imports_from_docs(
                    type_def.docs.contents.as_deref(),
                    origin_path,
                    &type_context,
                    &mut imports,
                )?;
                collect_language_imports_from_type_def(
                    type_def,
                    origin_path,
                    &type_context,
                    &mut imports,
                )?;
            }

            for function in interface.functions.values() {
                collect_language_imports_from_docs(
                    function.docs.contents.as_deref(),
                    origin_path,
                    &format!("interface `{interface_name}` function `{}`", function.name),
                    &mut imports,
                )?;
            }
        }
    }

    let mut imports_by_language = BTreeMap::<Language, Vec<LanguageImportSpec>>::new();
    for import in imports {
        imports_by_language
            .entry(import.language)
            .or_default()
            .push(import);
    }
    Ok(imports_by_language)
}

fn collect_language_imports_from_type_def(
    type_def: &TypeDef,
    origin_path: &Path,
    context: &str,
    imports: &mut BTreeSet<LanguageImportSpec>,
) -> Result<()> {
    match &type_def.kind {
        TypeDefKind::Record(record) => {
            for field in &record.fields {
                collect_language_imports_from_docs(
                    field.docs.contents.as_deref(),
                    origin_path,
                    &format!("{context} field `{}`", field.name),
                    imports,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_language_imports_from_docs(
    docs: Option<&str>,
    path: &Path,
    context: &str,
    imports: &mut BTreeSet<LanguageImportSpec>,
) -> Result<()> {
    for directive in parse_directives(docs, path, context)? {
        collect_typescript_imports_from_directive(&directive, path, context, imports)?;
        collect_python_imports_from_directive(&directive, imports)?;
    }
    Ok(())
}

fn collect_typescript_imports_from_directive(
    directive: &Directive,
    path: &Path,
    context: &str,
    imports: &mut BTreeSet<LanguageImportSpec>,
) -> Result<()> {
    let Some(package) = directive.value("typescript-package") else {
        return Ok(());
    };

    let expressions = typescript_import_expressions(directive);
    if expressions.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: format!("@nexus.{}", directive.name),
            reason: "`typescript-package` requires a TypeScript type expression".to_string(),
        });
    }

    let mut namespaces = BTreeSet::new();
    for expression in expressions {
        namespaces.extend(typescript_qualified_namespaces(expression));
    }

    match namespaces.len() {
        0 => Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: format!("@nexus.{}", directive.name),
            reason: "`typescript-package` requires a TypeScript type expression with a qualified namespace".to_string(),
        }),
        1 => {
            let namespace = namespaces.into_iter().next().expect("namespace count checked");
            imports.insert(LanguageImportSpec {
                language: Language::TypeScript,
                reference: namespace.clone(),
                module: package.to_string(),
                name: Some(namespace),
                type_only: true,
                import_style: if directive.name == "proto" {
                    LanguageImportStyle::Named
                } else {
                    LanguageImportStyle::Namespace
                },
            });
            Ok(())
        }
        _ => Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: format!("@nexus.{}", directive.name),
            reason: "multiple TypeScript namespaces in one annotated import are not supported"
                .to_string(),
        }),
    }
}

fn collect_python_imports_from_directive(
    directive: &Directive,
    imports: &mut BTreeSet<LanguageImportSpec>,
) -> Result<()> {
    for expression in python_import_expressions(directive) {
        for module_path in python_qualified_module_paths(expression) {
            imports.insert(LanguageImportSpec {
                language: Language::Python,
                reference: module_path.clone(),
                module: module_path,
                name: None,
                type_only: false,
                import_style: LanguageImportStyle::Module,
            });
        }
    }
    Ok(())
}

fn typescript_import_expressions(directive: &Directive) -> Vec<&str> {
    match directive.name.as_str() {
        "proto" => directive.value("value").into_iter().collect(),
        "type" | "flattened-type" => directive
            .value("typescript")
            .or_else(|| directive.value("value"))
            .into_iter()
            .collect(),
        "function" => directive
            .value("typescript-result")
            .or_else(|| directive.value("result"))
            .into_iter()
            .collect(),
        "typescript-with-arguments" => ["value-type", "args-type"]
            .into_iter()
            .filter_map(|key| directive.value(key))
            .collect(),
        "output-transform" => directive.value("typescript-type").into_iter().collect(),
        _ => Vec::new(),
    }
}

fn python_import_expressions(directive: &Directive) -> Vec<&str> {
    match directive.name.as_str() {
        "type" | "flattened-type" => directive
            .value("python")
            .or_else(|| directive.value("value"))
            .into_iter()
            .collect(),
        "function" => directive
            .value("python-result")
            .or_else(|| directive.value("result"))
            .into_iter()
            .collect(),
        "output-transform" => directive.value("python-type").into_iter().collect(),
        _ => Vec::new(),
    }
}

fn python_qualified_module_paths(expression: &str) -> BTreeSet<String> {
    let chars = expression.char_indices().collect::<Vec<_>>();
    let mut module_paths = BTreeSet::new();
    let mut index = 0;
    while index < chars.len() {
        let (start_byte, ch) = chars[index];
        if !is_python_identifier_start(ch) {
            index += 1;
            continue;
        }

        let before = expression[..start_byte].chars().next_back();
        if before.is_some_and(|before| is_python_identifier_char(before) || before == '.') {
            index += 1;
            continue;
        }

        let mut end = index + 1;
        while end < chars.len() {
            let ch = chars[end].1;
            if is_python_identifier_char(ch) || ch == '.' {
                end += 1;
            } else {
                break;
            }
        }
        let end_byte = chars
            .get(end)
            .map(|(byte, _)| *byte)
            .unwrap_or(expression.len());
        let qualified_name = expression[start_byte..end_byte].trim_end_matches('.');
        if let Some(module_path) = python_module_path_for_qualified_name(qualified_name) {
            module_paths.insert(module_path.to_string());
        }
        index = end;
    }
    module_paths
}

fn python_module_path_for_qualified_name(qualified_name: &str) -> Option<&str> {
    let (module_path, _) = qualified_name.rsplit_once('.')?;
    if is_builtin_python_import(module_path) {
        return None;
    }
    Some(module_path)
}

fn is_builtin_python_import(module_path: &str) -> bool {
    matches!(
        module_path,
        "collections.abc" | "typing" | "typing_extensions"
    )
}

fn is_python_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_python_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn typescript_qualified_namespaces(expression: &str) -> BTreeSet<String> {
    let chars = expression.char_indices().collect::<Vec<_>>();
    let mut namespaces = BTreeSet::new();
    let mut index = 0;
    while index < chars.len() {
        let (start_byte, ch) = chars[index];
        if !is_typescript_identifier_start(ch) {
            index += 1;
            continue;
        }

        let before = expression[..start_byte].chars().next_back();
        if before.is_some_and(|before| is_typescript_identifier_char(before) || before == '.') {
            index += 1;
            continue;
        }

        let mut end = index + 1;
        while end < chars.len() && is_typescript_identifier_char(chars[end].1) {
            end += 1;
        }
        let end_byte = chars
            .get(end)
            .map(|(byte, _)| *byte)
            .unwrap_or(expression.len());
        let mut after = end;
        while after < chars.len() && chars[after].1.is_whitespace() {
            after += 1;
        }
        if after < chars.len() && chars[after].1 == '.' {
            namespaces.insert(expression[start_byte..end_byte].to_string());
        }
        index = end;
    }
    namespaces
}

fn is_typescript_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_' || ch == '$'
}

fn is_typescript_identifier_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

struct PreparedWitWorkspace {
    temp_dir: TempDir,
    package_root: PathBuf,
}

fn prepare_wit_workspace(
    input: &str,
    path: &Path,
    linked_input_paths: &[PathBuf],
) -> Result<PreparedWitWorkspace> {
    let temp_dir = tempfile::tempdir().map_err(|source| Error::WriteFile {
        path: PathBuf::from("<tempdir>"),
        source,
    })?;
    let package_root = temp_dir.path().join("main");
    fs::create_dir_all(&package_root).map_err(|source| Error::WriteFile {
        path: package_root.clone(),
        source,
    })?;

    if let Some(source_dir) = input_package_source_dir(path) {
        copy_package_source_dir(&source_dir, &package_root, path)?;
    } else if let Some(source_dir) = input_support_source_dir(path) {
        copy_standalone_input_support_dir(&source_dir, &package_root, path)?;
    }

    let target_name = input_target_name(path);
    let target_path = package_root.join(&target_name);
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::WriteFile {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::write(&target_path, input).map_err(|source| Error::WriteFile {
        path: target_path,
        source,
    })?;

    copy_linked_inputs(&package_root, linked_input_paths)?;

    Ok(PreparedWitWorkspace {
        temp_dir,
        package_root,
    })
}

fn prepare_linked_metadata_workspace(input_paths: &[PathBuf]) -> Result<PreparedWitWorkspace> {
    let temp_dir = tempfile::tempdir().map_err(|source| Error::WriteFile {
        path: PathBuf::from("<tempdir>"),
        source,
    })?;
    let package_root = temp_dir.path().join("main");
    fs::create_dir_all(&package_root).map_err(|source| Error::WriteFile {
        path: package_root.clone(),
        source,
    })?;
    let stub_path = package_root.join("main.wit");
    fs::write(
        &stub_path,
        "package temporary:root@0.0.0;\n\nworld system {\n}\n",
    )
    .map_err(|source| Error::WriteFile {
        path: stub_path,
        source,
    })?;
    copy_linked_inputs(&package_root, input_paths)?;
    Ok(PreparedWitWorkspace {
        temp_dir,
        package_root,
    })
}

fn input_package_source_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return Some(path.to_path_buf());
    }

    if path.file_name()? != "main.wit" {
        return None;
    }

    let parent = path.parent()?;
    if parent.as_os_str().is_empty() || !parent.exists() {
        return None;
    }
    Some(parent.to_path_buf())
}

fn input_support_source_dir(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return None;
    }

    let parent = path.parent()?;
    if parent.as_os_str().is_empty() || !parent.exists() {
        return None;
    }
    Some(parent.to_path_buf())
}

fn input_target_name(path: &Path) -> OsString {
    path.file_name()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| OsString::from("input.wit"))
}

fn copy_package_source_dir(
    source_dir: &Path,
    destination_dir: &Path,
    input_path: &Path,
) -> Result<()> {
    for entry in fs::read_dir(source_dir).map_err(|source| Error::ReadFile {
        path: source_dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::ReadFile {
            path: source_dir.to_path_buf(),
            source,
        })?;
        let source_path = entry.path();
        let destination_path = destination_dir.join(entry.file_name());

        if source_path == input_path {
            continue;
        }

        let file_type = entry.file_type().map_err(|source| Error::ReadFile {
            path: source_path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            copy_package_source_dir(&source_path, &destination_path, input_path)?;
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::WriteFile {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::copy(&source_path, &destination_path).map_err(|source| Error::WriteFile {
            path: destination_path,
            source,
        })?;
    }

    Ok(())
}

fn copy_standalone_input_support_dir(
    source_dir: &Path,
    destination_dir: &Path,
    input_path: &Path,
) -> Result<()> {
    for entry in fs::read_dir(source_dir).map_err(|source| Error::ReadFile {
        path: source_dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::ReadFile {
            path: source_dir.to_path_buf(),
            source,
        })?;
        let source_path = entry.path();
        let destination_path = destination_dir.join(entry.file_name());

        if source_path == input_path {
            continue;
        }

        let file_type = entry.file_type().map_err(|source| Error::ReadFile {
            path: source_path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            if entry.file_name() == "deps" {
                continue;
            }
            copy_standalone_input_support_dir(&source_path, &destination_path, input_path)?;
            continue;
        }

        if source_path
            .extension()
            .is_some_and(|extension| extension == "wit")
        {
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::WriteFile {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::copy(&source_path, &destination_path).map_err(|source| Error::WriteFile {
            path: destination_path,
            source,
        })?;
    }

    Ok(())
}

fn copy_linked_inputs(package_root: &Path, linked_input_paths: &[PathBuf]) -> Result<()> {
    for linked_input_path in linked_input_paths {
        copy_linked_input(package_root, linked_input_path)?;
    }
    Ok(())
}

fn copy_linked_input(package_root: &Path, linked_input_path: &Path) -> Result<()> {
    let metadata = fs::metadata(linked_input_path).map_err(|source| Error::ReadFile {
        path: linked_input_path.to_path_buf(),
        source,
    })?;
    if metadata.is_file() {
        return copy_linked_input_file(package_root, linked_input_path);
    }
    if linked_input_path_is_package_dir(linked_input_path)? {
        return copy_linked_input_package_dir(package_root, linked_input_path);
    }
    copy_linked_input_collection_dir(package_root, linked_input_path)
}

fn copy_linked_input_file(package_root: &Path, linked_input_path: &Path) -> Result<()> {
    let package_name = linked_input_package_dir_name(linked_input_path)?;
    let destination_path = package_root
        .join("deps")
        .join(package_name)
        .join(input_target_name(linked_input_path));
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::WriteFile {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::copy(linked_input_path, &destination_path).map_err(|source| Error::WriteFile {
        path: destination_path,
        source,
    })?;
    Ok(())
}

fn copy_linked_input_package_dir(package_root: &Path, linked_input_path: &Path) -> Result<()> {
    let package_name = linked_input_package_dir_name(linked_input_path)?;
    let destination_path = package_root.join("deps").join(package_name);
    copy_directory_tree(linked_input_path, &destination_path)
}

fn copy_linked_input_collection_dir(package_root: &Path, linked_input_path: &Path) -> Result<()> {
    for entry in fs::read_dir(linked_input_path).map_err(|source| Error::ReadFile {
        path: linked_input_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::ReadFile {
            path: linked_input_path.to_path_buf(),
            source,
        })?;
        let source_path = entry.path();
        let file_type = entry.file_type().map_err(|source| Error::ReadFile {
            path: source_path.clone(),
            source,
        })?;
        if file_type.is_dir() {
            if linked_input_path_is_package_dir(&source_path)? {
                copy_linked_input_package_dir(package_root, &source_path)?;
            } else {
                copy_linked_input_collection_dir(package_root, &source_path)?;
            }
        } else if source_path
            .extension()
            .is_some_and(|extension| extension == "wit")
        {
            copy_linked_input_file(package_root, &source_path)?;
        }
    }
    Ok(())
}

fn linked_input_path_is_package_dir(linked_input_path: &Path) -> Result<bool> {
    for entry in fs::read_dir(linked_input_path).map_err(|source| Error::ReadFile {
        path: linked_input_path.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::ReadFile {
            path: linked_input_path.to_path_buf(),
            source,
        })?;
        let source_path = entry.path();
        if source_path
            .extension()
            .is_some_and(|extension| extension == "wit")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn linked_input_package_dir_name(linked_input_path: &Path) -> Result<OsString> {
    let file_name = linked_input_path
        .file_name()
        .ok_or_else(|| Error::InvalidWit {
            path: linked_input_path.to_path_buf(),
            reason: "linked WIT input path must name a package directory".to_string(),
        })?;
    if linked_input_path
        .extension()
        .is_some_and(|extension| extension == "wit")
    {
        return linked_input_path
            .file_stem()
            .map(|stem| stem.to_os_string())
            .ok_or_else(|| Error::InvalidWit {
                path: linked_input_path.to_path_buf(),
                reason: "linked WIT input file must have a stem".to_string(),
            });
    }
    Ok(file_name.to_os_string())
}

fn copy_directory_tree(source_dir: &Path, destination_dir: &Path) -> Result<()> {
    for entry in fs::read_dir(source_dir).map_err(|source| Error::ReadFile {
        path: source_dir.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| Error::ReadFile {
            path: source_dir.to_path_buf(),
            source,
        })?;
        let source_path = entry.path();
        let destination_path = destination_dir.join(entry.file_name());
        let file_type = entry.file_type().map_err(|source| Error::ReadFile {
            path: source_path.clone(),
            source,
        })?;

        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).map_err(|source| Error::WriteFile {
                path: destination_path.clone(),
                source,
            })?;
            copy_directory_tree(&source_path, &destination_path)?;
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::WriteFile {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::copy(&source_path, &destination_path).map_err(|source| Error::WriteFile {
            path: destination_path,
            source,
        })?;
    }
    Ok(())
}

fn collect_package_origins(
    resolve: &Resolve,
    source_map: &PackageSourceMap,
) -> Result<PackageOrigins> {
    let mut package_origins = BTreeMap::new();

    for (package_id, _) in resolve.packages.iter() {
        let Some(paths) = source_map.package_paths(package_id) else {
            continue;
        };
        let mut package_paths = paths.collect::<Vec<_>>();
        if package_paths.is_empty() {
            continue;
        }
        package_paths.sort();
        let origin = package_paths[0]
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        package_origins.insert(package_id, origin);
    }

    if package_origins.is_empty() {
        return Err(Error::InvalidWit {
            path: PathBuf::from("<workspace>"),
            reason: "resolved WIT package graph had no source origins".to_string(),
        });
    }

    Ok(package_origins)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationOutputTransformSpec {
    pub type_name: LanguageStringSpec,
    pub transform: LanguageStringSpec,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LanguageStringSpec {
    pub default: Option<String>,
    pub by_language: BTreeMap<Language, String>,
}

impl LanguageStringSpec {
    pub fn for_language(&self, language: Language) -> Option<&str> {
        self.by_language
            .get(&language)
            .or(self.default.as_ref())
            .map(String::as_str)
    }

    fn is_empty(&self) -> bool {
        self.default.is_none() && self.by_language.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeOverrideSpec {
    pub model_name: Option<String>,
    pub proto_type_name: LanguageStringSpec,
    pub required_fields: BTreeSet<String>,
    pub omitted_fields: BTreeSet<String>,
    pub replacement: Option<TypeReplacementSpec>,
    pub authored_type: Option<AuthoredFieldTypeSpec>,
    pub flatten_in_api: bool,
    pub experimental: bool,
    pub authored_record: bool,
    pub generated_model: GeneratedModelSpec,
}

impl TypeOverrideSpec {
    pub fn is_field_required(&self, field_name: &str) -> bool {
        self.required_fields.contains(field_name)
    }

    pub fn model_name(&self) -> Option<&str> {
        self.model_name.as_deref()
    }

    pub fn proto_type_name(&self) -> &LanguageStringSpec {
        &self.proto_type_name
    }

    pub fn is_field_omitted(&self, field_name: &str) -> bool {
        self.omitted_fields.contains(field_name)
    }

    pub fn is_field_hidden(&self, field_name: &str) -> bool {
        self.omitted_fields.contains(field_name) || self.field_source(field_name).is_some()
    }

    pub fn replacement(&self) -> Option<&TypeReplacementSpec> {
        self.replacement.as_ref()
    }

    pub fn flatten_in_api(&self) -> bool {
        self.flatten_in_api
    }

    pub fn experimental(&self) -> bool {
        self.experimental
    }

    pub fn generated_model(&self) -> Option<&GeneratedModelSpec> {
        if !self.authored_record && self.generated_model.is_empty() {
            None
        } else {
            Some(&self.generated_model)
        }
    }

    pub fn field_source(&self, field_name: &str) -> Option<&str> {
        self.generated_model()?.field_source(field_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeReplacementSpec {
    pub type_name: LanguageStringSpec,
    pub from_proto: LanguageStringSpec,
    pub to_proto: LanguageStringSpec,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneratedModelSpec {
    pub doc: LanguageStringSpec,
    pub declared_fields: Vec<String>,
    pub field_names: BTreeMap<String, String>,
    pub field_docs: BTreeMap<String, LanguageStringSpec>,
    pub field_annotations: BTreeMap<String, LanguageStringSpec>,
    pub field_flattened_annotations: BTreeMap<String, LanguageStringSpec>,
    pub field_wit_types: BTreeMap<String, AuthoredFieldTypeSpec>,
    pub field_defaults: BTreeMap<String, FieldDefaultSpec>,
    pub field_sources: BTreeMap<String, String>,
    pub functions: BTreeMap<String, FunctionFieldSpec>,
    pub with_arguments: BTreeMap<String, WithArgumentsFieldSpec>,
}

impl GeneratedModelSpec {
    pub fn is_empty(&self) -> bool {
        self.doc.is_empty()
            && self.declared_fields.is_empty()
            && self.field_names.is_empty()
            && self.field_docs.is_empty()
            && self.field_annotations.is_empty()
            && self.field_flattened_annotations.is_empty()
            && self.field_wit_types.is_empty()
            && self.field_defaults.is_empty()
            && self.field_sources.is_empty()
            && self.functions.is_empty()
            && self.with_arguments.is_empty()
    }

    pub fn field_name_override(&self, field_name: &str) -> Option<&str> {
        self.field_names.get(field_name).map(String::as_str)
    }

    pub fn doc(&self) -> &LanguageStringSpec {
        &self.doc
    }

    pub fn field_doc(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.field_docs.get(field_name)
    }

    pub fn field_annotation(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.field_annotations.get(field_name)
    }

    pub fn field_flattened_annotation(&self, field_name: &str) -> Option<&LanguageStringSpec> {
        self.field_flattened_annotations.get(field_name)
    }

    pub fn field_wit_type(&self, field_name: &str) -> Option<&AuthoredFieldTypeSpec> {
        self.field_wit_types.get(field_name)
    }

    pub fn field_default(&self, field_name: &str) -> Option<&FieldDefaultSpec> {
        self.field_defaults.get(field_name)
    }

    pub fn field_source(&self, field_name: &str) -> Option<&str> {
        self.field_sources.get(field_name).map(String::as_str)
    }

    pub fn function(&self, field_name: &str) -> Option<&FunctionFieldSpec> {
        self.functions.get(field_name)
    }

    pub fn function_for_args_field(&self, field_name: &str) -> Option<&FunctionFieldSpec> {
        self.functions.values().find(|function| {
            function
                .arg_fields
                .iter()
                .any(|arg_field| arg_field == field_name)
        })
    }

    pub fn with_arguments(&self, field_name: &str) -> Option<&WithArgumentsFieldSpec> {
        self.with_arguments.get(field_name)
    }

    pub fn with_arguments_for_args_field(
        &self,
        field_name: &str,
    ) -> Option<&WithArgumentsFieldSpec> {
        self.with_arguments
            .values()
            .find(|with_arguments| with_arguments.args_field == field_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefaultSpec {
    pub enum_case: String,
    pub enum_value: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthoredFieldTypeSpec {
    Bool,
    Int,
    Float,
    String,
    Bytes,
    Option(Box<AuthoredFieldTypeSpec>),
    List(Box<AuthoredFieldTypeSpec>),
    Tuple(Vec<AuthoredFieldTypeSpec>),
    Map(Box<AuthoredFieldTypeSpec>, Box<AuthoredFieldTypeSpec>),
    Result {
        ok: Option<Box<AuthoredFieldTypeSpec>>,
        err: Option<Box<AuthoredFieldTypeSpec>>,
    },
    Proto(String),
    Record(String),
    Enum(String),
    Flags(String),
    Variant(String),
    Resource(String),
    Alias {
        name: String,
        target: Box<AuthoredFieldTypeSpec>,
        type_name: LanguageStringSpec,
    },
}

impl AuthoredFieldTypeSpec {
    pub(crate) fn without_option(&self) -> &AuthoredFieldTypeSpec {
        match self {
            AuthoredFieldTypeSpec::Option(inner) => inner.without_option(),
            _ => self,
        }
    }

    pub(crate) fn validation_type(&self) -> &AuthoredFieldTypeSpec {
        match self {
            AuthoredFieldTypeSpec::Alias { target, .. } => target.validation_type(),
            _ => self,
        }
    }

    pub(crate) fn to_wit_string(&self) -> String {
        match self {
            AuthoredFieldTypeSpec::Bool => "bool".to_string(),
            AuthoredFieldTypeSpec::Int => "s64".to_string(),
            AuthoredFieldTypeSpec::Float => "float64".to_string(),
            AuthoredFieldTypeSpec::String => "string".to_string(),
            AuthoredFieldTypeSpec::Bytes => "bytes".to_string(),
            AuthoredFieldTypeSpec::Option(inner) => {
                format!("option<{}>", inner.to_wit_string())
            }
            AuthoredFieldTypeSpec::List(inner) => format!("list<{}>", inner.to_wit_string()),
            AuthoredFieldTypeSpec::Tuple(items) => format!(
                "tuple<{}>",
                items
                    .iter()
                    .map(AuthoredFieldTypeSpec::to_wit_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            AuthoredFieldTypeSpec::Map(key, value) => {
                format!("map<{}, {}>", key.to_wit_string(), value.to_wit_string())
            }
            AuthoredFieldTypeSpec::Result { ok, err } => match (ok, err) {
                (Some(ok), Some(err)) => {
                    format!("result<{}, {}>", ok.to_wit_string(), err.to_wit_string())
                }
                (Some(ok), None) => format!("result<{}>", ok.to_wit_string()),
                (None, Some(err)) => format!("result<_, {}>", err.to_wit_string()),
                (None, None) => "result".to_string(),
            },
            AuthoredFieldTypeSpec::Proto(proto_name) => proto_name.clone(),
            AuthoredFieldTypeSpec::Record(record_name) => record_name.clone(),
            AuthoredFieldTypeSpec::Enum(enum_name) => enum_name.clone(),
            AuthoredFieldTypeSpec::Flags(flags_name) => flags_name.clone(),
            AuthoredFieldTypeSpec::Variant(variant_name) => variant_name.clone(),
            AuthoredFieldTypeSpec::Resource(resource_name) => resource_name.clone(),
            AuthoredFieldTypeSpec::Alias { name, .. } => name.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionFieldSpec {
    pub primary: bool,
    pub result: FunctionResultSpec,
    pub args_field: String,
    pub arg_fields: Vec<String>,
    pub args: FunctionArgsSpec,
    pub alternate_type: Option<AuthoredFieldTypeSpec>,
    pub converter: Option<String>,
    pub name_extractor: Option<String>,
    pub result_type_parameter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionArgsSpec {
    Varargs {
        prefix: Vec<FunctionArgSpec>,
        typescript_drop_prefix: bool,
    },
    Fixed(Vec<FunctionArgSpec>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionArgSpec {
    pub name: String,
    pub field_type: AuthoredFieldTypeSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionResultSpec {
    Wit(AuthoredFieldTypeSpec),
    Annotation(LanguageStringSpec),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithArgumentsFieldSpec {
    pub args_field: String,
    pub value_type: String,
    pub args_type: String,
    pub name_expr: String,
    pub alternate_type: Option<AuthoredFieldTypeSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlattenedFunctionTypeSpec {
    arg_fields: Vec<FlattenedFunctionArgSpec>,
    function: Option<FunctionFieldSpec>,
    with_arguments: Option<WithArgumentsFieldSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlattenedFunctionArgSpec {
    args_name: String,
    field_name: String,
    field_type: AuthoredFieldTypeSpec,
    required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedFunctionSignature {
    args: ResolvedFunctionSignatureArgs,
    result: AuthoredFieldTypeSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedFunctionSignatureArgs {
    name: String,
    wit_type: AuthoredFieldTypeSpec,
    function_args: FunctionArgsSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionVarargsSpec {
    param: String,
    typescript_drop_prefix: bool,
}

fn collect_interface_types(
    resolve: &Resolve,
    interface: &Interface,
    path: &Path,
    language: Language,
    types: &mut BTreeMap<String, TypeOverrideSpec>,
    records: &mut BTreeMap<String, WitRecordSpec>,
    enums: &mut BTreeMap<String, WitEnumSpec>,
    flags: &mut BTreeMap<String, WitFlagsSpec>,
    variants: &mut BTreeMap<String, WitVariantSpec>,
) -> Result<()> {
    let interface_name = interface
        .name
        .as_deref()
        .unwrap_or("unnamed-interface")
        .to_string();
    for type_id in interface.types.values() {
        let type_def = &resolve.types[*type_id];
        if let Some(record) =
            build_wit_record_spec(resolve, *type_id, type_def, path, &interface_name, language)?
        {
            if records.insert(record.full_name.clone(), record).is_some() {
                return Err(Error::InvalidWit {
                    path: path.to_path_buf(),
                    reason: format!(
                        "duplicate WIT record mapping for `{}`",
                        wit_type_full_name(resolve, *type_id)
                    ),
                });
            }
        }
        if let Some(enumeration) = build_wit_enum_spec(resolve, *type_id, type_def) {
            if enums
                .insert(enumeration.full_name.clone(), enumeration)
                .is_some()
            {
                return Err(Error::InvalidWit {
                    path: path.to_path_buf(),
                    reason: format!(
                        "duplicate WIT enum mapping for `{}`",
                        wit_type_full_name(resolve, *type_id)
                    ),
                });
            }
        }
        if let Some(flag_set) = build_wit_flags_spec(resolve, *type_id, type_def) {
            if flags.insert(flag_set.full_name.clone(), flag_set).is_some() {
                return Err(Error::InvalidWit {
                    path: path.to_path_buf(),
                    reason: format!(
                        "duplicate WIT flags mapping for `{}`",
                        wit_type_full_name(resolve, *type_id)
                    ),
                });
            }
        }
        if let Some(variant) = build_wit_variant_spec(resolve, *type_id, type_def, path)? {
            if variants
                .insert(variant.full_name.clone(), variant)
                .is_some()
            {
                return Err(Error::InvalidWit {
                    path: path.to_path_buf(),
                    reason: format!(
                        "duplicate WIT variant mapping for `{}`",
                        wit_type_full_name(resolve, *type_id)
                    ),
                });
            }
        }
        let Some((proto_name, type_override)) =
            build_type_override(resolve, type_def, path, &interface_name, language)?
        else {
            continue;
        };
        if types.insert(proto_name.clone(), type_override).is_some() {
            return Err(Error::InvalidWit {
                path: path.to_path_buf(),
                reason: format!("duplicate `@nexus.proto` mapping for `{proto_name}`"),
            });
        }
    }

    Ok(())
}

fn build_wit_enum_spec(
    resolve: &Resolve,
    type_id: TypeId,
    type_def: &TypeDef,
) -> Option<WitEnumSpec> {
    let TypeDefKind::Enum(enumeration) = &type_def.kind else {
        return None;
    };

    let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
    Some(WitEnumSpec {
        name: type_name.to_upper_camel_case(),
        full_name: wit_type_full_name(resolve, type_id),
        values: enumeration
            .cases
            .iter()
            .enumerate()
            .map(|(index, value)| WitEnumValueSpec {
                wit_name: value.name.clone(),
                name: value.name.to_upper_camel_case(),
                number: i32::try_from(index).expect("WIT enum case index should fit in i32"),
            })
            .collect(),
    })
}

fn build_wit_flags_spec(
    resolve: &Resolve,
    type_id: TypeId,
    type_def: &TypeDef,
) -> Option<WitFlagsSpec> {
    let TypeDefKind::Flags(flags) = &type_def.kind else {
        return None;
    };

    let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
    Some(WitFlagsSpec {
        name: type_name.to_upper_camel_case(),
        full_name: wit_type_full_name(resolve, type_id),
        flags: flags
            .flags
            .iter()
            .enumerate()
            .map(|(index, flag)| WitFlagSpec {
                name: flag.name.to_upper_camel_case(),
                bit: index,
            })
            .collect(),
    })
}

fn build_wit_variant_spec(
    resolve: &Resolve,
    type_id: TypeId,
    type_def: &TypeDef,
    path: &Path,
) -> Result<Option<WitVariantSpec>> {
    let TypeDefKind::Variant(variant) = &type_def.kind else {
        return Ok(None);
    };

    let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
    let context = format!("type `{}`", wit_type_full_name(resolve, type_id));
    Ok(Some(WitVariantSpec {
        name: type_name.to_upper_camel_case(),
        full_name: wit_type_full_name(resolve, type_id),
        cases: variant
            .cases
            .iter()
            .map(|case| {
                Ok(WitVariantCaseSpec {
                    name: case.name.clone(),
                    payload: case
                        .ty
                        .as_ref()
                        .map(|ty| {
                            resolve_authored_field_type_spec(
                                resolve,
                                ty,
                                path,
                                &format!("{context} case `{}`", case.name),
                            )
                        })
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    }))
}

fn build_wit_record_spec(
    resolve: &Resolve,
    type_id: TypeId,
    type_def: &TypeDef,
    path: &Path,
    interface_name: &str,
    language: Language,
) -> Result<Option<WitRecordSpec>> {
    let TypeDefKind::Record(record) = &type_def.kind else {
        return Ok(None);
    };

    let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
    let context = format!("type `{interface_name}.{type_name}`");
    let directives = parse_directives(type_def.docs.contents.as_deref(), path, &context)?;
    let experimental = experimental_directive(&directives, path, &context)?;
    let model_doc = directive(&directives, "doc", path, &context)?
        .map(directive_language_string)
        .unwrap_or_default();
    let (required_fields, _omitted_fields, generated_model) = build_generated_model_from_record(
        resolve,
        type_def.owner,
        record,
        model_doc,
        path,
        &context,
        language,
    )?;

    Ok(Some(WitRecordSpec {
        name: type_name.to_upper_camel_case(),
        full_name: wit_type_full_name(resolve, type_id),
        experimental,
        required_fields,
        generated_model,
    }))
}

fn wit_type_full_name(resolve: &Resolve, type_id: TypeId) -> String {
    let type_def = &resolve.types[type_id];
    let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
    match type_def.owner {
        TypeOwner::Interface(interface_id) => {
            let interface = &resolve.interfaces[interface_id];
            let interface_name = interface.name.as_deref().unwrap_or("unnamed-interface");
            format!("{interface_name}.{type_name}")
        }
        TypeOwner::World(world_id) => {
            let world = &resolve.worlds[world_id];
            let world_name = world.name.as_str();
            format!("{world_name}.{type_name}")
        }
        TypeOwner::None => type_name.to_string(),
    }
}

fn build_type_override(
    resolve: &Resolve,
    type_def: &TypeDef,
    path: &Path,
    interface_name: &str,
    language: Language,
) -> Result<Option<(String, TypeOverrideSpec)>> {
    let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
    let context = format!("type `{interface_name}.{type_name}`");
    let directives = parse_directives(type_def.docs.contents.as_deref(), path, &context)?;
    let Some(proto_directive) = directive(&directives, "proto", path, &context)? else {
        return Ok(None);
    };
    let Some(proto_name) = proto_directive.value("value").map(ToOwned::to_owned) else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context,
            directive: "@nexus.proto".to_string(),
            reason: "missing required proto type name".to_string(),
        });
    };
    let proto_type_name = directive_prefixed_language_string(proto_directive, "type");

    let replacement = build_type_replacement(&directives, path, &context, &proto_name)?;

    let flatten_in_api = directive(&directives, "flatten-in-api", path, &context)?.is_some();
    let experimental = experimental_directive(&directives, path, &context)?;
    let authored_record = matches!(type_def.kind, TypeDefKind::Record(_));
    if flatten_in_api && !authored_record {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.clone(),
            directive: "@nexus.flatten-in-api".to_string(),
            reason: "only supported on record types".to_string(),
        });
    }

    if directive(&directives, "omit", path, &context)?.is_some() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.clone(),
            directive: "@nexus.omit".to_string(),
            reason: "mark record fields with `@nexus.omit`; type-level omit is no longer supported"
                .to_string(),
        });
    }

    let (required_fields, omitted_fields, generated_model) = match &type_def.kind {
        TypeDefKind::Record(record) => {
            let model_doc = directive(&directives, "doc", path, &context)?
                .map(directive_language_string)
                .unwrap_or_default();
            build_generated_model_from_record(
                resolve,
                type_def.owner,
                record,
                model_doc,
                path,
                &context,
                language,
            )?
        }
        _ => (
            BTreeSet::new(),
            BTreeSet::new(),
            GeneratedModelSpec::default(),
        ),
    };

    let type_override = TypeOverrideSpec {
        model_name: authored_record.then(|| type_name.to_upper_camel_case()),
        proto_type_name,
        required_fields,
        omitted_fields,
        replacement,
        authored_type: resolve_authored_type_def_kind(resolve, type_def, path, &context)?,
        flatten_in_api,
        experimental,
        authored_record,
        generated_model,
    };

    Ok(Some((proto_name, type_override)))
}

fn build_generated_model_from_record(
    resolve: &Resolve,
    owner: TypeOwner,
    record: &wit_parser::Record,
    doc: LanguageStringSpec,
    path: &Path,
    context: &str,
    language: Language,
) -> Result<(BTreeSet<String>, BTreeSet<String>, GeneratedModelSpec)> {
    let mut required_fields = BTreeSet::new();
    let mut omitted_fields = BTreeSet::new();
    let mut authored_proto_fields = BTreeSet::new();
    let mut declared_fields = Vec::new();
    let mut field_names = BTreeMap::new();
    let mut field_docs = BTreeMap::new();
    let mut field_annotations = BTreeMap::new();
    let mut field_flattened_annotations = BTreeMap::new();
    let mut field_wit_types = BTreeMap::new();
    let mut field_defaults = BTreeMap::new();
    let mut field_sources = BTreeMap::new();
    let mut functions = BTreeMap::new();
    let mut with_arguments = BTreeMap::new();

    for field in &record.fields {
        let field_context = format!("{context} field `{}`", field.name);
        let directives = parse_directives(field.docs.contents.as_deref(), path, &field_context)?;
        if directive(&directives, "with-arguments", path, &field_context)?.is_some() {
            return Err(Error::InvalidWitDirective {
                path: path.to_path_buf(),
                context: field_context,
                directive: "@nexus.with-arguments".to_string(),
                reason: "renamed to `@nexus.typescript-with-arguments`".to_string(),
            });
        }
        let omit_directive = directive(&directives, "omit", path, &field_context)?;
        let proto_field_name =
            directive_value(&directives, "proto-field", path, &field_context, "value")?
                .unwrap_or_else(|| field.name.to_snake_case());
        let function_directive = directive(&directives, "function", path, &field_context)?;
        let default_directive = directive(&directives, "default", path, &field_context)?;
        if default_directive.is_some()
            && directive(&directives, "source", path, &field_context)?.is_some()
        {
            return Err(Error::InvalidWitDirective {
                path: path.to_path_buf(),
                context: field_context,
                directive: "@nexus.default".to_string(),
                reason: "cannot be combined with `@nexus.source`".to_string(),
            });
        }
        let typescript_with_arguments_directive = directive(
            &directives,
            "typescript-with-arguments",
            path,
            &field_context,
        )?;
        let flattened_function_type = if omit_directive.is_none()
            && function_directive.is_none()
            && typescript_with_arguments_directive.is_none()
        {
            find_flattened_function_type_spec(resolve, &field.ty, path, language)?
        } else {
            None
        };

        if !authored_proto_fields.insert(proto_field_name.clone()) {
            return Err(Error::InvalidWit {
                path: path.to_path_buf(),
                reason: format!(
                    "{field_context} maps to duplicate proto field `{proto_field_name}`"
                ),
            });
        }

        if let Some(omit_directive) = omit_directive {
            if !omit_directive.args.is_empty() {
                return Err(Error::InvalidWitDirective {
                    path: path.to_path_buf(),
                    context: field_context,
                    directive: "@nexus.omit".to_string(),
                    reason: "field-level omit does not take arguments".to_string(),
                });
            }

            for conflicting_directive in [
                "source",
                "type",
                "flattened-type",
                "function",
                "default",
                "typescript-with-arguments",
            ] {
                if directive(&directives, conflicting_directive, path, &field_context)?.is_some() {
                    return Err(Error::InvalidWitDirective {
                        path: path.to_path_buf(),
                        context: field_context,
                        directive: "@nexus.omit".to_string(),
                        reason: format!("cannot be combined with `@nexus.{conflicting_directive}`"),
                    });
                }
            }

            if flattened_function_type.is_some() {
                return Err(Error::InvalidWitDirective {
                    path: path.to_path_buf(),
                    context: field_context,
                    directive: "@nexus.omit".to_string(),
                    reason: "cannot be combined with a flattened function field type".to_string(),
                });
            }

            omitted_fields.insert(proto_field_name);
            continue;
        }

        declared_fields.push(proto_field_name.clone());

        field_names.insert(proto_field_name.clone(), field.name.clone());
        if let Some(doc_directive) = directive(&directives, "doc", path, &field_context)? {
            let doc = directive_language_string(doc_directive);
            if !doc.is_empty() {
                field_docs.insert(proto_field_name.clone(), doc);
            }
        }
        let field_wit_type =
            resolve_authored_field_type_spec(resolve, &field.ty, path, &field_context)?;
        let field_default =
            build_field_default(resolve, &field.ty, default_directive, path, &field_context)?;
        field_wit_types.insert(proto_field_name.clone(), field_wit_type);

        if !is_optional_type(resolve, &field.ty) && field_default.is_none() {
            required_fields.insert(proto_field_name.clone());
        }

        if let Some(source) = build_source_call(&directives, path, &field_context, language)? {
            field_sources.insert(proto_field_name.clone(), source);
        }

        if let Some(field_default) = field_default {
            field_defaults.insert(proto_field_name.clone(), field_default);
        }

        if let Some(type_directive) = directive(&directives, "type", path, &field_context)? {
            field_annotations.insert(
                proto_field_name.clone(),
                directive_language_string(type_directive),
            );
        }

        if let Some(flattened_type_directive) =
            directive(&directives, "flattened-type", path, &field_context)?
        {
            field_flattened_annotations.insert(
                proto_field_name.clone(),
                directive_language_string(flattened_type_directive),
            );
        }

        if let Some(function) =
            build_function_field(resolve, owner, &directives, path, &field_context, language)?
        {
            functions.insert(proto_field_name.clone(), function);
        }

        if let Some(with_arguments_field) =
            build_with_arguments_field(resolve, owner, &directives, path, &field_context)?
        {
            with_arguments.insert(proto_field_name.clone(), with_arguments_field);
        }

        if field_sources.contains_key(&proto_field_name)
            && functions.contains_key(&proto_field_name)
        {
            return Err(Error::ConflictingTypeOverrideFieldProperties {
                message: context.to_string(),
                field: proto_field_name,
                property: "source",
                conflicting_property: "function",
            });
        }

        if let Some(flattened_function_type) = flattened_function_type {
            for arg_field in flattened_function_type.arg_fields {
                if !authored_proto_fields.insert(arg_field.field_name.clone()) {
                    return Err(Error::InvalidWit {
                        path: path.to_path_buf(),
                        reason: format!(
                            "{field_context} maps to duplicate proto field `{}`",
                            arg_field.field_name
                        ),
                    });
                }
                declared_fields.push(arg_field.field_name.clone());
                field_names.insert(arg_field.field_name.clone(), arg_field.args_name);
                if arg_field.required {
                    required_fields.insert(arg_field.field_name.clone());
                }
                field_wit_types.insert(arg_field.field_name, arg_field.field_type);
            }
            if let Some(function) = flattened_function_type.function {
                functions.insert(proto_field_name.clone(), function);
            }
            if let Some(with_arguments_field) = flattened_function_type.with_arguments {
                with_arguments.insert(proto_field_name.clone(), with_arguments_field);
            }
        }
    }

    Ok((
        required_fields,
        omitted_fields,
        GeneratedModelSpec {
            doc,
            declared_fields,
            field_names,
            field_docs,
            field_annotations,
            field_flattened_annotations,
            field_wit_types,
            field_defaults,
            field_sources,
            functions,
            with_arguments,
        },
    ))
}

fn build_type_replacement(
    directives: &[Directive],
    path: &Path,
    context: &str,
    type_name: &str,
) -> Result<Option<TypeReplacementSpec>> {
    let directive = directive(directives, "type", path, context)?;
    let Some(directive) = directive else {
        return Ok(None);
    };

    let type_name_spec = directive_language_string(directive);
    let from_proto = directive_prefixed_language_string(directive, "from");
    let to_proto = directive_prefixed_language_string(directive, "to");
    if type_name_spec.is_empty() {
        if !from_proto.is_empty() || !to_proto.is_empty() {
            return Err(Error::IncompleteTypeOverride {
                type_name: type_name.to_string(),
            });
        }
        return Ok(None);
    }
    Ok(Some(TypeReplacementSpec {
        type_name: type_name_spec,
        from_proto,
        to_proto,
    }))
}

fn resolve_authored_field_type_spec(
    resolve: &Resolve,
    ty: &Type,
    path: &Path,
    context: &str,
) -> Result<AuthoredFieldTypeSpec> {
    match ty {
        Type::Bool => Ok(AuthoredFieldTypeSpec::Bool),
        Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64 => Ok(AuthoredFieldTypeSpec::Int),
        Type::F32 | Type::F64 => Ok(AuthoredFieldTypeSpec::Float),
        Type::Char | Type::String => Ok(AuthoredFieldTypeSpec::String),
        Type::Id(id) => {
            let type_def = &resolve.types[*id];
            let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
            let type_context = format!("{context} type `{type_name}`");
            let directives =
                parse_directives(type_def.docs.contents.as_deref(), path, &type_context)?;
            if let Some(proto_name) = find_proto_name_for_type_def(type_def, path, &type_context)? {
                if let Some(type_directive) = directive(&directives, "type", path, &type_context)? {
                    return Ok(AuthoredFieldTypeSpec::Alias {
                        name: wit_type_full_name(resolve, *id),
                        target: Box::new(AuthoredFieldTypeSpec::Proto(proto_name)),
                        type_name: directive_language_string(type_directive),
                    });
                }
                return Ok(AuthoredFieldTypeSpec::Proto(proto_name));
            }
            match &type_def.kind {
                TypeDefKind::Option(inner) => Ok(AuthoredFieldTypeSpec::Option(Box::new(
                    resolve_authored_field_type_spec(resolve, inner, path, &type_context)?,
                ))),
                TypeDefKind::List(inner) => Ok(AuthoredFieldTypeSpec::List(Box::new(
                    resolve_authored_field_type_spec(resolve, inner, path, &type_context)?,
                ))),
                TypeDefKind::Tuple(tuple) => Ok(AuthoredFieldTypeSpec::Tuple(
                    tuple
                        .types
                        .iter()
                        .map(|item| {
                            resolve_authored_field_type_spec(resolve, item, path, &type_context)
                        })
                        .collect::<Result<Vec<_>>>()?,
                )),
                TypeDefKind::Map(key, value) => Ok(AuthoredFieldTypeSpec::Map(
                    Box::new(resolve_authored_field_type_spec(
                        resolve,
                        key,
                        path,
                        &type_context,
                    )?),
                    Box::new(resolve_authored_field_type_spec(
                        resolve,
                        value,
                        path,
                        &type_context,
                    )?),
                )),
                TypeDefKind::Result(result) => Ok(AuthoredFieldTypeSpec::Result {
                    ok: result
                        .ok
                        .as_ref()
                        .map(|ok| {
                            resolve_authored_field_type_spec(resolve, ok, path, &type_context)
                                .map(Box::new)
                        })
                        .transpose()?,
                    err: result
                        .err
                        .as_ref()
                        .map(|err| {
                            resolve_authored_field_type_spec(resolve, err, path, &type_context)
                                .map(Box::new)
                        })
                        .transpose()?,
                }),
                TypeDefKind::Type(next) => {
                    let target =
                        resolve_authored_field_type_spec(resolve, next, path, &type_context)?;
                    if let Some(type_directive) =
                        directive(&directives, "type", path, &type_context)?
                    {
                        Ok(AuthoredFieldTypeSpec::Alias {
                            name: wit_type_full_name(resolve, *id),
                            target: Box::new(target),
                            type_name: directive_language_string(type_directive),
                        })
                    } else if let Some(function_directive) =
                        directive(&directives, "function", path, &type_context)?
                    {
                        if let Some(type_name) = function_alias_type_name(
                            resolve,
                            type_def,
                            function_directive,
                            path,
                            &type_context,
                        )? {
                            Ok(AuthoredFieldTypeSpec::Alias {
                                name: wit_type_full_name(resolve, *id),
                                target: Box::new(target),
                                type_name,
                            })
                        } else {
                            Ok(target)
                        }
                    } else {
                        Ok(target)
                    }
                }
                TypeDefKind::Record(_) => Ok(AuthoredFieldTypeSpec::Record(wit_type_full_name(
                    resolve, *id,
                ))),
                TypeDefKind::Enum(_) => Ok(AuthoredFieldTypeSpec::Enum(wit_type_full_name(
                    resolve, *id,
                ))),
                TypeDefKind::Flags(_) => Ok(AuthoredFieldTypeSpec::Flags(wit_type_full_name(
                    resolve, *id,
                ))),
                TypeDefKind::Variant(_) => Ok(AuthoredFieldTypeSpec::Variant(wit_type_full_name(
                    resolve, *id,
                ))),
                TypeDefKind::Handle(Handle::Own(resource_id))
                | TypeDefKind::Handle(Handle::Borrow(resource_id)) => {
                    let resource_def = &resolve.types[*resource_id];
                    let resource_name = resource_def.name.as_deref().unwrap_or("unnamed-resource");
                    Ok(AuthoredFieldTypeSpec::Resource(resource_name.to_string()))
                }
                TypeDefKind::Resource => Ok(AuthoredFieldTypeSpec::Resource(type_name.to_string())),
                _ => Err(Error::InvalidWit {
                    path: path.to_path_buf(),
                    reason: format!(
                        "{type_context} uses unsupported WIT type `{}` for generated model fields",
                        type_def.kind.as_str()
                    ),
                }),
            }
        }
        _ => Err(Error::InvalidWit {
            path: path.to_path_buf(),
            reason: format!("{context} uses unsupported WIT type for generated model fields"),
        }),
    }
}

pub(crate) fn find_proto_name_for_type(
    resolve: &Resolve,
    ty: &Type,
    path: &Path,
    context: &str,
) -> Result<Option<String>> {
    let mut current = ty;
    loop {
        match current {
            Type::Id(id) => {
                let type_def = &resolve.types[*id];
                let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
                let type_context = format!("{context} type `{type_name}`");
                let directives =
                    parse_directives(type_def.docs.contents.as_deref(), path, &type_context)?;
                if let Some(proto_name) =
                    directive_value(&directives, "proto", path, &type_context, "value")?
                {
                    return Ok(Some(proto_name));
                }
                match &type_def.kind {
                    TypeDefKind::Type(next) => current = next,
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        }
    }
}

fn find_owned_resource_name_for_type(resolve: &Resolve, ty: &Type) -> Option<String> {
    match ty {
        Type::Id(id) => find_owned_resource_name_for_type_def(resolve, &resolve.types[*id]),
        _ => None,
    }
}

fn find_owned_resource_name_for_type_def(resolve: &Resolve, type_def: &TypeDef) -> Option<String> {
    match &type_def.kind {
        TypeDefKind::Handle(Handle::Own(resource_id)) => resolve.types[*resource_id]
            .name
            .as_deref()
            .map(str::to_string),
        TypeDefKind::Type(next) => find_owned_resource_name_for_type(resolve, next),
        _ => None,
    }
}

fn resolve_authored_type_def_kind(
    resolve: &Resolve,
    type_def: &TypeDef,
    path: &Path,
    context: &str,
) -> Result<Option<AuthoredFieldTypeSpec>> {
    match &type_def.kind {
        TypeDefKind::Type(next) => Ok(Some(resolve_authored_field_type_spec(
            resolve, next, path, context,
        )?)),
        TypeDefKind::Option(inner) => Ok(Some(AuthoredFieldTypeSpec::Option(Box::new(
            resolve_authored_field_type_spec(resolve, inner, path, context)?,
        )))),
        TypeDefKind::List(inner) => Ok(Some(AuthoredFieldTypeSpec::List(Box::new(
            resolve_authored_field_type_spec(resolve, inner, path, context)?,
        )))),
        TypeDefKind::Tuple(tuple) => Ok(Some(AuthoredFieldTypeSpec::Tuple(
            tuple
                .types
                .iter()
                .map(|item| resolve_authored_field_type_spec(resolve, item, path, context))
                .collect::<Result<Vec<_>>>()?,
        ))),
        TypeDefKind::Map(key, value) => Ok(Some(AuthoredFieldTypeSpec::Map(
            Box::new(resolve_authored_field_type_spec(
                resolve, key, path, context,
            )?),
            Box::new(resolve_authored_field_type_spec(
                resolve, value, path, context,
            )?),
        ))),
        TypeDefKind::Result(result) => Ok(Some(AuthoredFieldTypeSpec::Result {
            ok: result
                .ok
                .as_ref()
                .map(|ok| {
                    resolve_authored_field_type_spec(resolve, ok, path, context).map(Box::new)
                })
                .transpose()?,
            err: result
                .err
                .as_ref()
                .map(|err| {
                    resolve_authored_field_type_spec(resolve, err, path, context).map(Box::new)
                })
                .transpose()?,
        })),
        _ => Ok(None),
    }
}

fn find_wit_record_name_for_type(resolve: &Resolve, ty: &Type) -> Option<String> {
    match ty {
        Type::Id(id) => find_wit_record_name_for_type_def(resolve, *id, &resolve.types[*id]),
        _ => None,
    }
}

fn find_wit_record_name_for_type_def(
    resolve: &Resolve,
    type_id: TypeId,
    type_def: &TypeDef,
) -> Option<String> {
    match &type_def.kind {
        TypeDefKind::Record(_) => Some(wit_type_full_name(resolve, type_id)),
        TypeDefKind::Type(next) => find_wit_record_name_for_type(resolve, next),
        _ => None,
    }
}

pub(crate) fn find_proto_name_for_type_def(
    type_def: &TypeDef,
    path: &Path,
    context: &str,
) -> Result<Option<String>> {
    let directives = parse_directives(type_def.docs.contents.as_deref(), path, context)?;
    directive_value(&directives, "proto", path, context, "value")
}

fn build_field_default(
    resolve: &Resolve,
    ty: &Type,
    directive: Option<&Directive>,
    path: &Path,
    context: &str,
) -> Result<Option<FieldDefaultSpec>> {
    let Some(directive) = directive else {
        return Ok(None);
    };
    let Some(case_name) = directive.value("value") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.default".to_string(),
            reason: "missing required default enum case".to_string(),
        });
    };
    if case_name.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.default".to_string(),
            reason: "default enum case cannot be empty".to_string(),
        });
    }
    let enum_value = resolve_default_enum_value(resolve, ty, case_name, path, context)?;
    Ok(Some(FieldDefaultSpec {
        enum_case: case_name.to_string(),
        enum_value,
    }))
}

fn resolve_default_enum_value(
    resolve: &Resolve,
    ty: &Type,
    case_name: &str,
    path: &Path,
    context: &str,
) -> Result<i32> {
    match ty {
        Type::Id(id) => resolve_default_enum_value_for_type_def(
            resolve,
            *id,
            &resolve.types[*id],
            case_name,
            path,
            context,
        ),
        _ => Err(default_on_non_enum_error(path, context)),
    }
}

fn resolve_default_enum_value_for_type_def(
    resolve: &Resolve,
    type_id: TypeId,
    type_def: &TypeDef,
    case_name: &str,
    path: &Path,
    context: &str,
) -> Result<i32> {
    match &type_def.kind {
        TypeDefKind::Enum(enumeration) => {
            for (index, case) in enumeration.cases.iter().enumerate() {
                if case.name == case_name {
                    return i32::try_from(index).map_err(|_| Error::InvalidWitDirective {
                        path: path.to_path_buf(),
                        context: context.to_string(),
                        directive: "@nexus.default".to_string(),
                        reason: "enum case index does not fit in i32".to_string(),
                    });
                }
            }
            Err(Error::InvalidWitDirective {
                path: path.to_path_buf(),
                context: context.to_string(),
                directive: "@nexus.default".to_string(),
                reason: format!(
                    "unknown enum case `{case_name}` for `{}`",
                    wit_type_full_name(resolve, type_id)
                ),
            })
        }
        TypeDefKind::Option(inner) | TypeDefKind::Type(inner) => {
            resolve_default_enum_value(resolve, inner, case_name, path, context)
        }
        _ => Err(default_on_non_enum_error(path, context)),
    }
}

fn default_on_non_enum_error(path: &Path, context: &str) -> Error {
    Error::InvalidWitDirective {
        path: path.to_path_buf(),
        context: context.to_string(),
        directive: "@nexus.default".to_string(),
        reason: "only enum field defaults are supported".to_string(),
    }
}

fn build_source_call(
    directives: &[Directive],
    path: &Path,
    context: &str,
    language: Language,
) -> Result<Option<String>> {
    let Some(directive) = directive(directives, "source", path, context)? else {
        return Ok(None);
    };

    let Some(helper_name) =
        directive_language_value(directive, language).or_else(|| directive.value("value"))
    else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.source".to_string(),
            reason: "missing required support helper name".to_string(),
        });
    };

    if !is_valid_support_helper_path(helper_name) {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.source".to_string(),
            reason: format!("invalid support helper name `{helper_name}`"),
        });
    }

    Ok(Some(format!("{helper_name}()")))
}

fn is_valid_support_helper_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_valid_support_helper_path(name: &str) -> bool {
    name.split('.').all(is_valid_support_helper_name)
}

fn build_function_field(
    resolve: &Resolve,
    owner: TypeOwner,
    directives: &[Directive],
    path: &Path,
    context: &str,
    language: Language,
) -> Result<Option<FunctionFieldSpec>> {
    let Some(directive) = directive(directives, "function", path, context)? else {
        return Ok(None);
    };

    let result = directive_result_language_string(directive);
    if result.is_empty() {
        return Ok(None);
    }

    let Some(args_field) = directive.value("args-field") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason: "missing required `args-field`".to_string(),
        });
    };

    let primary = directive
        .value("primary")
        .map(parse_bool)
        .transpose()
        .map_err(|reason| Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason,
        })?
        .unwrap_or(false);

    Ok(Some(FunctionFieldSpec {
        primary,
        result: FunctionResultSpec::Annotation(result),
        args_field: args_field.to_snake_case(),
        arg_fields: vec![args_field.to_snake_case()],
        args: FunctionArgsSpec::Varargs {
            prefix: Vec::new(),
            typescript_drop_prefix: false,
        },
        alternate_type: function_alternate_type(resolve, owner, directive, path, context)?,
        converter: directive_converter(directive, language),
        name_extractor: directive_function_name_extractor(directive, language, path, context)?,
        result_type_parameter: directive_result_type_parameter(directive),
    }))
}

fn build_function_field_for_type_alias(
    resolve: &Resolve,
    type_def: &TypeDef,
    directives: &[Directive],
    path: &Path,
    context: &str,
    language: Language,
) -> Result<Option<(String, AuthoredFieldTypeSpec, FunctionFieldSpec)>> {
    let Some(function_directive) = directive(directives, "function", path, context)? else {
        return Ok(None);
    };

    let primary = function_directive
        .value("primary")
        .map(parse_bool)
        .transpose()
        .map_err(|reason| Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason,
        })?
        .unwrap_or(false);

    let converter = directive_converter(function_directive, language);

    if let Some(signature_name) = function_directive.value("signature") {
        if function_directive.value("args-name").is_some()
            || !directive_result_language_string(function_directive).is_empty()
        {
            return Err(Error::InvalidWitDirective {
                path: path.to_path_buf(),
                context: context.to_string(),
                directive: "@nexus.function".to_string(),
                reason: "signature cannot be combined with args-name or result overrides"
                    .to_string(),
            });
        }

        let signature =
            resolve_function_signature(resolve, type_def, signature_name, path, context)?;
        let args_name = signature.args.name;
        let args_type = signature.args.wit_type;
        let function_args = signature.args.function_args;
        let args_field = function_directive
            .value("args-field")
            .unwrap_or(&args_name)
            .to_snake_case();
        let arg_fields = function_arg_fields(&function_args, &args_field);
        return Ok(Some((
            args_name.clone(),
            args_type,
            FunctionFieldSpec {
                primary,
                result: FunctionResultSpec::Wit(signature.result),
                args_field,
                arg_fields,
                args: function_args,
                alternate_type: function_alternate_type(
                    resolve,
                    type_def.owner,
                    function_directive,
                    path,
                    context,
                )?,
                converter,
                name_extractor: directive_function_name_extractor(
                    function_directive,
                    language,
                    path,
                    context,
                )?,
                result_type_parameter: directive_result_type_parameter(function_directive),
            },
        )));
    }

    let result = directive_result_language_string(function_directive);
    if result.is_empty() {
        return Ok(None);
    }

    Err(Error::InvalidWitDirective {
        path: path.to_path_buf(),
        context: context.to_string(),
        directive: "@nexus.function".to_string(),
        reason: "type-level function annotations must use `signature`".to_string(),
    })
}

fn function_alternate_type(
    resolve: &Resolve,
    owner: TypeOwner,
    directive: &Directive,
    path: &Path,
    context: &str,
) -> Result<Option<AuthoredFieldTypeSpec>> {
    directive
        .value("alternate-type")
        .map(|type_name| {
            resolve_named_wit_type(resolve, owner, type_name, path, context, "@nexus.function")
        })
        .transpose()
}

fn function_arg_fields(function_args: &FunctionArgsSpec, args_field: &str) -> Vec<String> {
    match function_args {
        FunctionArgsSpec::Varargs { .. } => vec![args_field.to_string()],
        FunctionArgsSpec::Fixed(args) => args.iter().map(|arg| arg.name.to_snake_case()).collect(),
    }
}

fn build_with_arguments_field(
    resolve: &Resolve,
    owner: TypeOwner,
    directives: &[Directive],
    path: &Path,
    context: &str,
) -> Result<Option<WithArgumentsFieldSpec>> {
    let Some(directive) = directive(directives, "typescript-with-arguments", path, context)? else {
        return Ok(None);
    };

    let Some(args_field) = directive.value("args-field") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `args-field`".to_string(),
        });
    };
    let Some(value_type) = directive.value("value-type") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `value-type`".to_string(),
        });
    };
    let Some(args_type) = directive.value("args-type") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `args-type`".to_string(),
        });
    };
    let Some(name_expr) = directive.value("name-expr") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `name-expr`".to_string(),
        });
    };

    Ok(Some(WithArgumentsFieldSpec {
        args_field: args_field.to_snake_case(),
        value_type: value_type.to_string(),
        args_type: args_type.to_string(),
        name_expr: name_expr.to_string(),
        alternate_type: with_arguments_alternate_type(resolve, owner, directive, path, context)?,
    }))
}

fn build_with_arguments_field_for_type_alias(
    resolve: &Resolve,
    type_def: &TypeDef,
    directives: &[Directive],
    path: &Path,
    context: &str,
) -> Result<Option<(String, AuthoredFieldTypeSpec, WithArgumentsFieldSpec)>> {
    let Some(directive) = directive(directives, "typescript-with-arguments", path, context)? else {
        return Ok(None);
    };

    let (args_name, args_wit_type) = if let Some(signature_name) = directive.value("signature") {
        if directive.value("args-name").is_some() {
            return Err(Error::InvalidWitDirective {
                path: path.to_path_buf(),
                context: context.to_string(),
                directive: "@nexus.typescript-with-arguments".to_string(),
                reason: "signature cannot be combined with args-name".to_string(),
            });
        }
        let (args_name, args_type) =
            resolve_function_signature_args(resolve, type_def, signature_name, path, context)?;
        (args_name, args_type)
    } else if directive.value("args-name").is_some() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "type-level with-arguments annotations must use `signature`".to_string(),
        });
    } else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `args-name` or `signature`".to_string(),
        });
    };

    let Some(value_type) = directive.value("value-type") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `value-type`".to_string(),
        });
    };
    let Some(args_type) = directive.value("args-type") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `args-type`".to_string(),
        });
    };
    let Some(name_expr) = directive.value("name-expr") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "missing required `name-expr`".to_string(),
        });
    };

    Ok(Some((
        args_name.clone(),
        args_wit_type,
        WithArgumentsFieldSpec {
            args_field: directive
                .value("args-field")
                .unwrap_or(&args_name)
                .to_snake_case(),
            value_type: value_type.to_string(),
            args_type: args_type.to_string(),
            name_expr: name_expr.to_string(),
            alternate_type: with_arguments_alternate_type(
                resolve,
                type_def.owner,
                directive,
                path,
                context,
            )?,
        },
    )))
}

fn with_arguments_alternate_type(
    resolve: &Resolve,
    owner: TypeOwner,
    directive: &Directive,
    path: &Path,
    context: &str,
) -> Result<Option<AuthoredFieldTypeSpec>> {
    directive
        .value("alternate-type")
        .map(|type_name| {
            resolve_named_wit_type(
                resolve,
                owner,
                type_name,
                path,
                context,
                "@nexus.typescript-with-arguments",
            )
        })
        .transpose()
}

fn flattened_function_arg_fields(
    function: &FunctionFieldSpec,
    args_name: &str,
    args_type: &AuthoredFieldTypeSpec,
) -> Vec<FlattenedFunctionArgSpec> {
    match &function.args {
        FunctionArgsSpec::Varargs { .. } => vec![FlattenedFunctionArgSpec {
            args_name: args_name.to_string(),
            field_name: function.args_field.clone(),
            field_type: args_type.clone(),
            required: false,
        }],
        FunctionArgsSpec::Fixed(args) => args
            .iter()
            .map(|arg| FlattenedFunctionArgSpec {
                args_name: arg.name.clone(),
                field_name: arg.name.to_snake_case(),
                field_type: arg.field_type.clone(),
                required: true,
            })
            .collect(),
    }
}

fn find_flattened_function_type_spec(
    resolve: &Resolve,
    ty: &Type,
    path: &Path,
    language: Language,
) -> Result<Option<FlattenedFunctionTypeSpec>> {
    let mut current = ty;
    loop {
        match current {
            Type::Id(id) => {
                let type_def = &resolve.types[*id];
                let type_name = type_def.name.as_deref().unwrap_or("unnamed-type");
                let context = format!("type `{type_name}`");
                let directives =
                    parse_directives(type_def.docs.contents.as_deref(), path, &context)?;
                let function = build_function_field_for_type_alias(
                    resolve,
                    type_def,
                    &directives,
                    path,
                    &context,
                    language,
                )?;
                let with_arguments = build_with_arguments_field_for_type_alias(
                    resolve,
                    type_def,
                    &directives,
                    path,
                    &context,
                )?;
                if function.is_some() || with_arguments.is_some() {
                    let arg_fields = match (&function, &with_arguments) {
                        (
                            Some((function_args_name, function_args_type, function)),
                            Some((
                                with_arguments_args_name,
                                with_arguments_args_type,
                                with_arguments,
                            )),
                        ) => {
                            if function_args_name != with_arguments_args_name {
                                return Err(Error::InvalidWitDirective {
                                    path: path.to_path_buf(),
                                    context,
                                    directive: "@nexus.typescript-with-arguments".to_string(),
                                    reason: format!(
                                        "args-name `{with_arguments_args_name}` does not match function signature args-name `{function_args_name}`"
                                    ),
                                });
                            }
                            if function.args_field != with_arguments.args_field {
                                return Err(Error::InvalidWitDirective {
                                    path: path.to_path_buf(),
                                    context,
                                    directive: "@nexus.typescript-with-arguments".to_string(),
                                    reason: format!(
                                        "args-field `{}` does not match function args-field `{}`",
                                        with_arguments.args_field, function.args_field
                                    ),
                                });
                            }
                            if function_args_type != with_arguments_args_type {
                                return Err(Error::InvalidWitDirective {
                                    path: path.to_path_buf(),
                                    context,
                                    directive: "@nexus.typescript-with-arguments".to_string(),
                                    reason: format!(
                                        "args type `{}` does not match function args type `{}`",
                                        with_arguments_args_type.to_wit_string(),
                                        function_args_type.to_wit_string()
                                    ),
                                });
                            }
                            flattened_function_arg_fields(
                                function,
                                function_args_name,
                                function_args_type,
                            )
                        }
                        (Some((args_name, args_type, function)), None) => {
                            flattened_function_arg_fields(function, args_name, args_type)
                        }
                        (None, Some((args_name, args_type, with_arguments))) => {
                            vec![FlattenedFunctionArgSpec {
                                args_name: args_name.clone(),
                                field_name: with_arguments.args_field.clone(),
                                field_type: args_type.clone(),
                                required: false,
                            }]
                        }
                        (None, None) => unreachable!("checked for a present flattened function"),
                    };
                    return Ok(Some(FlattenedFunctionTypeSpec {
                        arg_fields,
                        function: function.map(|(_, _, function)| function),
                        with_arguments: with_arguments.map(|(_, _, with_arguments)| with_arguments),
                    }));
                }
                match &type_def.kind {
                    TypeDefKind::Type(next) => current = next,
                    _ => return Ok(None),
                }
            }
            _ => return Ok(None),
        }
    }
}

fn function_alias_type_name(
    resolve: &Resolve,
    type_def: &TypeDef,
    function_directive: &Directive,
    path: &Path,
    context: &str,
) -> Result<Option<LanguageStringSpec>> {
    let result = if let Some(signature_name) = function_directive.value("signature") {
        resolve_function_signature(resolve, type_def, signature_name, path, context)?.result
    } else {
        let result = directive_result_language_string(function_directive);
        if result.is_empty() {
            return Ok(None);
        }
        AuthoredFieldTypeSpec::Alias {
            name: String::new(),
            target: Box::new(AuthoredFieldTypeSpec::String),
            type_name: result,
        }
    };
    let mut result_type = authored_type_language_string(&result);
    if result_type.is_empty() {
        return Ok(None);
    }
    if let Some(type_parameter) = directive_result_type_parameter(function_directive) {
        replace_type_parameter_for_language(
            &mut result_type,
            Language::Python,
            &type_parameter,
            "typing.Any",
        );
        replace_type_parameter_for_language(
            &mut result_type,
            Language::TypeScript,
            &type_parameter,
            "any",
        );
    }

    let mut type_name = LanguageStringSpec::default();
    if let Some(result_type) = result_type.for_language(Language::Python) {
        type_name.by_language.insert(
            Language::Python,
            format!("str | collections.abc.Callable[..., {result_type}]"),
        );
    }
    if let Some(result_type) = result_type.for_language(Language::TypeScript) {
        type_name.by_language.insert(
            Language::TypeScript,
            format!("string | ((...args: any[]) => {result_type})"),
        );
    }
    Ok((!type_name.is_empty()).then_some(type_name))
}

fn authored_type_language_string(wit_type: &AuthoredFieldTypeSpec) -> LanguageStringSpec {
    match wit_type {
        AuthoredFieldTypeSpec::Alias {
            type_name, target, ..
        } => {
            if type_name.is_empty() {
                authored_type_language_string(target)
            } else {
                type_name.clone()
            }
        }
        _ => LanguageStringSpec::default(),
    }
}

fn replace_type_parameter_for_language(
    spec: &mut LanguageStringSpec,
    language: Language,
    type_parameter: &str,
    replacement: &str,
) {
    if let Some(value) = spec.by_language.get_mut(&language) {
        *value = value.replace(type_parameter, replacement);
    }
}

fn resolve_function_signature(
    resolve: &Resolve,
    type_def: &TypeDef,
    signature_name: &str,
    path: &Path,
    context: &str,
) -> Result<ResolvedFunctionSignature> {
    let TypeOwner::Interface(interface_id) = type_def.owner else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason: "signature is only supported on interface-owned types".to_string(),
        });
    };
    let interface = &resolve.interfaces[interface_id];
    let Some(function) = interface.functions.get(signature_name) else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason: format!("unknown signature `{signature_name}`"),
        });
    };
    let args = resolve_function_signature_args_for_function(
        resolve,
        function,
        path,
        context,
        signature_name,
    )?;
    let function_context = format!("{context} signature `{signature_name}`");
    let Some(result_type) = &function.result else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason: format!("signature `{signature_name}` must declare a result type"),
        });
    };

    Ok(ResolvedFunctionSignature {
        args,
        result: resolve_authored_field_type_spec(resolve, result_type, path, &function_context)?,
    })
}

fn resolve_named_wit_type(
    resolve: &Resolve,
    owner: TypeOwner,
    type_name: &str,
    path: &Path,
    context: &str,
    directive_name: &str,
) -> Result<AuthoredFieldTypeSpec> {
    if let Some(primitive) = primitive_wit_type(type_name) {
        return Ok(primitive);
    }
    let TypeOwner::Interface(interface_id) = owner else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: directive_name.to_string(),
            reason: "`alternate-type` is only supported for interface-owned types".to_string(),
        });
    };
    let interface = &resolve.interfaces[interface_id];
    let Some(type_id) = interface.types.get(type_name) else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: directive_name.to_string(),
            reason: format!("unknown alternate type `{type_name}`"),
        });
    };
    resolve_authored_field_type_spec(resolve, &Type::Id(*type_id), path, context)
}

fn primitive_wit_type(type_name: &str) -> Option<AuthoredFieldTypeSpec> {
    match type_name {
        "bool" => Some(AuthoredFieldTypeSpec::Bool),
        "u8" | "u16" | "u32" | "u64" | "s8" | "s16" | "s32" | "s64" => {
            Some(AuthoredFieldTypeSpec::Int)
        }
        "f32" | "f64" => Some(AuthoredFieldTypeSpec::Float),
        "char" | "string" => Some(AuthoredFieldTypeSpec::String),
        "bytes" => Some(AuthoredFieldTypeSpec::Bytes),
        _ => None,
    }
}

fn resolve_function_signature_args(
    resolve: &Resolve,
    type_def: &TypeDef,
    signature_name: &str,
    path: &Path,
    context: &str,
) -> Result<(String, AuthoredFieldTypeSpec)> {
    let TypeOwner::Interface(interface_id) = type_def.owner else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: "signature is only supported on interface-owned types".to_string(),
        });
    };
    let interface = &resolve.interfaces[interface_id];
    let Some(function) = interface.functions.get(signature_name) else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.typescript-with-arguments".to_string(),
            reason: format!("unknown signature `{signature_name}`"),
        });
    };

    let args = resolve_function_signature_args_for_function(
        resolve,
        function,
        path,
        context,
        signature_name,
    )?;
    Ok((args.name, args.wit_type))
}

fn resolve_function_signature_args_for_function(
    resolve: &Resolve,
    function: &Function,
    path: &Path,
    context: &str,
    signature_name: &str,
) -> Result<ResolvedFunctionSignatureArgs> {
    if !matches!(
        function.kind,
        FunctionKind::Freestanding | FunctionKind::AsyncFreestanding
    ) {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason: format!(
                "signature `{signature_name}` must be a freestanding interface function"
            ),
        });
    }
    if function.params.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason: format!("signature `{signature_name}` must have at least one parameter"),
        });
    }

    let function_args = function
        .params
        .iter()
        .map(|param| {
            let function_context = format!(
                "{context} signature `{signature_name}` parameter `{}`",
                param.name
            );
            Ok(FunctionArgSpec {
                name: param.name.clone(),
                field_type: resolve_authored_field_type_spec(
                    resolve,
                    &param.ty,
                    path,
                    &function_context,
                )?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let varargs = function_varargs(function, path, context, signature_name)?;
    let wit_type = if let Some(varargs) = &varargs {
        function_args
            .iter()
            .find(|arg| arg.name == varargs.param)
            .map(|arg| arg.field_type.clone())
            .expect("validated varargs param should exist")
    } else {
        function_args_field_type(&function_args)
    };
    Ok(ResolvedFunctionSignatureArgs {
        name: if let Some(varargs) = varargs.clone() {
            varargs.param
        } else if function.params.len() == 1 {
            function_args[0].name.clone()
        } else {
            "args".to_string()
        },
        function_args: if let Some(varargs) = varargs {
            let varargs_index = function_args
                .iter()
                .position(|arg| arg.name == varargs.param)
                .expect("validated varargs param should exist");
            FunctionArgsSpec::Varargs {
                prefix: function_args[..varargs_index].to_vec(),
                typescript_drop_prefix: varargs.typescript_drop_prefix,
            }
        } else {
            FunctionArgsSpec::Fixed(function_args)
        },
        wit_type,
    })
}

fn function_varargs(
    function: &Function,
    path: &Path,
    context: &str,
    signature_name: &str,
) -> Result<Option<FunctionVarargsSpec>> {
    let function_context = format!("{context} signature `{signature_name}`");
    let directives = parse_directives(function.docs.contents.as_deref(), path, &function_context)?;
    let Some(directive) = directive(&directives, "function-args", path, &function_context)? else {
        return Ok(None);
    };
    let varargs = directive
        .value("varargs")
        .map(parse_bool)
        .transpose()
        .map_err(|reason| Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: function_context.clone(),
            directive: "@nexus.function-args".to_string(),
            reason,
        })?
        .unwrap_or(false);
    if !varargs {
        return Ok(None);
    }
    let typescript_drop_prefix = directive
        .value("typescript-drop-prefix")
        .map(parse_bool)
        .transpose()
        .map_err(|reason| Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: function_context.clone(),
            directive: "@nexus.function-args".to_string(),
            reason,
        })?
        .unwrap_or(false);
    let param_name = if let Some(param_name) = directive.value("param") {
        param_name.to_string()
    } else if function.params.len() == 1 {
        function.params[0].name.clone()
    } else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: function_context,
            directive: "@nexus.function-args".to_string(),
            reason:
                "`param` is required when varargs is used on a signature with multiple parameters"
                    .to_string(),
        });
    };
    if !function.params.iter().any(|param| param.name == param_name) {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: function_context,
            directive: "@nexus.function-args".to_string(),
            reason: format!("unknown varargs parameter `{param_name}`"),
        });
    }
    if function
        .params
        .last()
        .is_none_or(|param| param.name != param_name)
    {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: function_context,
            directive: "@nexus.function-args".to_string(),
            reason: format!("varargs parameter `{param_name}` must be the final parameter"),
        });
    }
    Ok(Some(FunctionVarargsSpec {
        param: param_name,
        typescript_drop_prefix,
    }))
}

fn function_args_field_type(args: &[FunctionArgSpec]) -> AuthoredFieldTypeSpec {
    if let Some(first) = args.first()
        && args.iter().all(|arg| arg.field_type == first.field_type)
    {
        return AuthoredFieldTypeSpec::List(Box::new(first.field_type.clone()));
    }
    AuthoredFieldTypeSpec::Tuple(
        args.iter()
            .map(|arg| arg.field_type.clone())
            .collect::<Vec<_>>(),
    )
}

fn build_service(
    resolve: &Resolve,
    key: &WorldKey,
    interface: &Interface,
    path: &Path,
    language: Language,
) -> Result<ServiceSpec> {
    let interface_name = interface_export_name(key, interface);
    let context = format!("interface `{interface_name}`");
    let directives = parse_directives(interface.docs.contents.as_deref(), path, &context)?;
    let endpoint = directive_value(&directives, "endpoint", path, &context, "value")?;
    let service_name = interface_name.to_upper_camel_case();
    let wire_service_name = build_wire_service_name(&directives, path, &context, &service_name)?;
    let experimental = experimental_directive(&directives, path, &context)?;
    let delay_load_temporalio_workflow =
        delay_load_temporalio_workflow_directive(&directives, path, &context)?;

    let operations = interface
        .functions
        .iter()
        .filter(|(_, function)| {
            matches!(
                function.kind,
                FunctionKind::Freestanding | FunctionKind::AsyncFreestanding
            )
        })
        .map(|(_, function)| build_operation(resolve, function, path, &context, &service_name))
        .collect::<Result<Vec<_>>>()?;
    ensure_unique_wire_operation_names(path, &context, &operations)?;

    let mut resources = Vec::new();
    for type_id in interface.types.values() {
        let type_def = &resolve.types[*type_id];
        if !matches!(type_def.kind, TypeDefKind::Resource) {
            continue;
        }
        resources.push(build_resource(
            resolve, interface, *type_id, type_def, path, &context, language,
        )?);
    }

    Ok(ServiceSpec {
        name: service_name,
        wire_name: wire_service_name,
        endpoint,
        experimental,
        delay_load_temporalio_workflow,
        operations,
        resources,
    })
}

fn build_wire_service_name(
    directives: &[Directive],
    path: &Path,
    context: &str,
    default_wire_service_name: &str,
) -> Result<String> {
    let Some(directive) = directive(directives, "service-name", path, context)? else {
        return Ok(default_wire_service_name.to_string());
    };
    let Some(name) = directive.value("name").or_else(|| directive.value("value")) else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.service-name".to_string(),
            reason: "missing required `name`".to_string(),
        });
    };
    if name.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.service-name".to_string(),
            reason: "`name` cannot be empty".to_string(),
        });
    }
    Ok(name.to_string())
}

fn ensure_unique_wire_operation_names(
    path: &Path,
    context: &str,
    operations: &[OperationSpec],
) -> Result<()> {
    let mut seen = BTreeMap::<String, String>::new();
    for operation in operations {
        if let Some(existing) = seen.insert(operation.wire_name.clone(), operation.name.clone()) {
            return Err(Error::InvalidWit {
                path: path.to_path_buf(),
                reason: format!(
                    "{context} operations `{existing}` and `{}` both use Nexus operation name `{}`",
                    operation.name, operation.wire_name
                ),
            });
        }
    }
    Ok(())
}

fn build_resource(
    resolve: &Resolve,
    interface: &Interface,
    resource_id: wit_parser::TypeId,
    type_def: &TypeDef,
    path: &Path,
    service_context: &str,
    language: Language,
) -> Result<ResourceSpec> {
    let resource_name = type_def
        .name
        .as_deref()
        .ok_or_else(|| Error::InvalidWit {
            path: path.to_path_buf(),
            reason: format!("{service_context} declares an unnamed resource"),
        })?
        .to_string();
    let context = format!(
        "{service_context} resource `{}`",
        resource_name.to_upper_camel_case()
    );

    let constructor = interface.functions.values().find(
        |function| matches!(function.kind, FunctionKind::Constructor(id) if id == resource_id),
    );
    let fields = match constructor {
        Some(constructor) => constructor
            .params
            .iter()
            .map(|param| {
                build_resource_field(
                    resolve,
                    &param.name,
                    &param.ty,
                    path,
                    &context,
                    "constructor",
                    language,
                )
            })
            .collect::<Result<Vec<_>>>()?,
        None => Vec::new(),
    };

    let methods = interface
        .functions
        .values()
        .filter(|function| {
            matches!(
                function.kind,
                FunctionKind::Method(id) | FunctionKind::AsyncMethod(id) if id == resource_id
            )
        })
        .map(|function| build_resource_method(resolve, function, path, &context, language))
        .collect::<Result<Vec<_>>>()?;

    for function in interface.functions.values() {
        match function.kind {
            FunctionKind::Static(id) | FunctionKind::AsyncStatic(id) if id == resource_id => {
                return Err(Error::InvalidWit {
                    path: path.to_path_buf(),
                    reason: format!(
                        "{context} static methods are not supported yet (`{}`)",
                        function.name
                    ),
                });
            }
            _ => {}
        }
    }

    Ok(ResourceSpec {
        name: resource_name,
        fields,
        methods,
    })
}

fn build_resource_method(
    resolve: &Resolve,
    function: &Function,
    path: &Path,
    resource_context: &str,
    language: Language,
) -> Result<ResourceMethodSpec> {
    let method_name = function
        .name
        .rsplit('.')
        .next()
        .unwrap_or(function.name.as_str())
        .to_string();
    let context = format!(
        "{resource_context} method `{}`",
        method_name.to_upper_camel_case()
    );
    let directives = parse_directives(function.docs.contents.as_deref(), path, &context)?;
    let params = function
        .params
        .iter()
        .skip_while(|param| param.name == "self")
        .map(|param| {
            build_resource_field(
                resolve,
                &param.name,
                &param.ty,
                path,
                &context,
                "parameter",
                language,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let result = function
        .result
        .as_ref()
        .map(|ty| build_resource_result(resolve, ty, path, &context))
        .transpose()?;

    Ok(ResourceMethodSpec {
        name: method_name,
        params,
        result,
        operation_name: build_resource_method_operation_name(&directives, path, &context)?,
    })
}

fn build_resource_method_operation_name(
    directives: &[Directive],
    path: &Path,
    context: &str,
) -> Result<Option<String>> {
    let Some(directive) = directive(directives, "operation", path, context)? else {
        return Ok(None);
    };
    let Some(name) = directive.value("name").or_else(|| directive.value("value")) else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.operation".to_string(),
            reason: "missing required `name`".to_string(),
        });
    };
    if name.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.operation".to_string(),
            reason: "`name` cannot be empty".to_string(),
        });
    }
    Ok(Some(name.to_upper_camel_case()))
}

fn build_resource_result(
    resolve: &Resolve,
    ty: &Type,
    path: &Path,
    context: &str,
) -> Result<ResourceResultSpec> {
    Ok(ResourceResultSpec {
        result_type: resolve_authored_field_type_spec(resolve, ty, path, context)?,
        proto: find_proto_name_for_type(resolve, ty, path, context)?,
        resource: find_owned_resource_name_for_type(resolve, ty),
    })
}

fn build_resource_field(
    resolve: &Resolve,
    name: &str,
    ty: &Type,
    path: &Path,
    context: &str,
    _role: &str,
    language: Language,
) -> Result<ResourceFieldSpec> {
    let field_type = resolve_authored_field_type_spec(resolve, ty, path, context)?;
    let function = find_flattened_function_type_spec(resolve, ty, path, language)?
        .and_then(|function_type| function_type.function);
    Ok(ResourceFieldSpec {
        name: name.to_string(),
        optional: is_optional_type(resolve, ty),
        field_type,
        function,
    })
}

fn build_operation(
    resolve: &Resolve,
    function: &Function,
    path: &Path,
    service_context: &str,
    service_name: &str,
) -> Result<OperationSpec> {
    let operation_name = function.name.to_upper_camel_case();
    let context = format!("{service_context} operation `{operation_name}`");
    let directives = parse_directives(function.docs.contents.as_deref(), path, &context)?;
    let wire_operation_name =
        build_wire_operation_name(&directives, path, &context, &operation_name)?;
    let experimental = experimental_directive(&directives, path, &context)?;

    let [parameter] = function.params.as_slice() else {
        return Err(Error::InvalidWit {
            path: path.to_path_buf(),
            reason: format!("{context} must declare exactly one input parameter"),
        });
    };
    let parameter_name = &parameter.name;
    let input_type = &parameter.ty;
    let input_proto = find_proto_name_for_type(resolve, input_type, path, &context)?;
    let input_record = find_wit_record_name_for_type(resolve, input_type);
    if input_proto.is_none() && input_record.is_none() {
        return Err(Error::InvalidWit {
            path: path.to_path_buf(),
            reason: format!(
                "{context} parameter `{parameter_name}` type must resolve to a WIT record or a type annotated with `@nexus.proto`"
            ),
        });
    }
    let output_transform = build_operation_output_transform(
        &directives,
        path,
        &context,
        service_name,
        &operation_name,
    )?;
    let (output_proto, output_record, output_resource) = if let Some(output_type) =
        function.result.as_ref()
    {
        let output_proto = find_proto_name_for_type(resolve, output_type, path, &context)?;
        let output_record = find_wit_record_name_for_type(resolve, output_type);
        let output_resource = find_owned_resource_name_for_type(resolve, output_type);
        if output_proto.is_none() && output_record.is_none() && output_resource.is_none() {
            return Err(Error::InvalidWit {
                path: path.to_path_buf(),
                reason: format!(
                    "{context} result type must resolve to a WIT record, resource, or type annotated with `@nexus.proto`"
                ),
            });
        }
        (output_proto, output_record, output_resource)
    } else {
        if output_transform.is_some() {
            return Err(Error::InvalidWitDirective {
                path: path.to_path_buf(),
                context,
                directive: "@nexus.output-transform".to_string(),
                reason: "operation does not declare a result type".to_string(),
            });
        }
        (None, None, None)
    };

    Ok(OperationSpec {
        name: operation_name,
        wire_name: wire_operation_name,
        experimental,
        doc: directive(&directives, "doc", path, &context)?
            .map(directive_language_string)
            .unwrap_or_default(),
        return_doc: directive(&directives, "doc", path, &context)?
            .map(directive_returns_language_string)
            .unwrap_or_default(),
        input_proto: input_proto.unwrap_or_default(),
        output_proto: output_proto.unwrap_or_default(),
        input_record,
        output_record,
        output_resource,
        output_transform,
    })
}

fn build_wire_operation_name(
    directives: &[Directive],
    path: &Path,
    context: &str,
    default_wire_operation_name: &str,
) -> Result<String> {
    let Some(directive) = directive(directives, "operation", path, context)? else {
        return Ok(default_wire_operation_name.to_string());
    };
    let Some(name) = directive.value("name").or_else(|| directive.value("value")) else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.operation".to_string(),
            reason: "missing required `name`".to_string(),
        });
    };
    if name.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.operation".to_string(),
            reason: "`name` cannot be empty".to_string(),
        });
    }
    Ok(name.to_string())
}

pub(crate) fn wire_operation_name_from_docs(
    docs: Option<&str>,
    path: &Path,
    context: &str,
    default_wire_operation_name: &str,
) -> Result<String> {
    let directives = parse_directives(docs, path, context)?;
    build_wire_operation_name(&directives, path, context, default_wire_operation_name)
}

fn build_operation_output_transform(
    directives: &[Directive],
    path: &Path,
    context: &str,
    service_name: &str,
    operation_name: &str,
) -> Result<Option<OperationOutputTransformSpec>> {
    let Some(directive) = directive(directives, "output-transform", path, context)? else {
        return Ok(None);
    };

    let type_name = directive_prefixed_language_string(directive, "type");
    let transform = directive_language_string(directive);

    if type_name.is_empty() && transform.is_empty() {
        Ok(None)
    } else if !type_name.is_empty() && !transform.is_empty() {
        Ok(Some(OperationOutputTransformSpec {
            type_name,
            transform,
        }))
    } else {
        Err(Error::IncompleteOperationOutputTransform {
            service: service_name.to_string(),
            operation: operation_name.to_string(),
        })
    }
}

pub(crate) fn select_world(
    resolve: &Resolve,
    package_id: PackageId,
    path: &Path,
) -> Result<wit_parser::WorldId> {
    let package = &resolve.packages[package_id];
    match package.worlds.len() {
        1 => Ok(*package
            .worlds
            .values()
            .next()
            .expect("world map length checked")),
        0 => Err(Error::InvalidWit {
            path: path.to_path_buf(),
            reason: "package must declare exactly one world".to_string(),
        }),
        _ => Err(Error::InvalidWit {
            path: path.to_path_buf(),
            reason: "package declares multiple worlds; choose one world per input".to_string(),
        }),
    }
}

fn interface_export_name(key: &WorldKey, interface: &Interface) -> String {
    match key {
        WorldKey::Name(name) => name.clone(),
        WorldKey::Interface(_) => interface
            .name
            .clone()
            .unwrap_or_else(|| "unnamed-interface".to_string()),
    }
}

fn is_optional_type(resolve: &Resolve, ty: &Type) -> bool {
    let mut current = ty;
    loop {
        match current {
            Type::Id(id) => match &resolve.types[*id].kind {
                TypeDefKind::Option(_) => return true,
                TypeDefKind::Type(next) => current = next,
                _ => return false,
            },
            _ => return false,
        }
    }
}

fn directive_value_for_language(
    directives: &[Directive],
    name: &str,
    path: &Path,
    context: &str,
    language: Language,
) -> Result<Option<String>> {
    let Some(directive) = directive(directives, name, path, context)? else {
        return Ok(None);
    };
    Ok(directive_language_value(directive, language)
        .or_else(|| directive.value("value"))
        .map(ToOwned::to_owned))
}

fn directive_language_string(directive: &Directive) -> LanguageStringSpec {
    let mut spec = LanguageStringSpec {
        default: directive.value("value").map(ToOwned::to_owned),
        by_language: BTreeMap::new(),
    };
    for language in all_languages() {
        if let Some(value) = directive.value(language_key(language)) {
            spec.by_language.insert(language, value.to_string());
        }
    }
    spec
}

fn directive_prefixed_language_string(directive: &Directive, suffix: &str) -> LanguageStringSpec {
    let mut spec = LanguageStringSpec::default();
    for language in all_languages() {
        if let Some(value) = directive.value(&format!("{}-{suffix}", language_key(language))) {
            spec.by_language.insert(language, value.to_string());
        }
    }
    spec
}

fn directive_returns_language_string(directive: &Directive) -> LanguageStringSpec {
    let mut spec = directive_prefixed_language_string(directive, "returns");
    spec.default = directive.value("returns").map(ToOwned::to_owned);
    spec
}

fn directive_result_language_string(directive: &Directive) -> LanguageStringSpec {
    let mut spec = directive_prefixed_language_string(directive, "result");
    spec.default = directive.value("result").map(ToOwned::to_owned);
    spec
}

fn directive_converter(directive: &Directive, language: Language) -> Option<String> {
    let mut spec = directive_prefixed_language_string(directive, "converter");
    spec.default = directive.value("converter").map(ToOwned::to_owned);
    spec.for_language(language).map(ToOwned::to_owned)
}

fn directive_function_name_extractor(
    directive: &Directive,
    language: Language,
    path: &Path,
    context: &str,
) -> Result<Option<String>> {
    let mut spec = directive_prefixed_language_string(directive, "name-extractor");
    spec.default = directive.value("name-extractor").map(ToOwned::to_owned);
    let Some(extractor) = spec.for_language(language) else {
        return Ok(None);
    };
    if !is_valid_support_helper_path(extractor) {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.function".to_string(),
            reason: format!(
                "invalid `{}` name-extractor `{extractor}`",
                language_key(language)
            ),
        });
    }
    Ok(Some(extractor.to_string()))
}

fn directive_result_type_parameter(directive: &Directive) -> Option<String> {
    directive
        .value("result-type-parameter")
        .map(ToOwned::to_owned)
}

fn directive_value(
    directives: &[Directive],
    name: &str,
    path: &Path,
    context: &str,
    key: &str,
) -> Result<Option<String>> {
    Ok(directive(directives, name, path, context)?
        .and_then(|directive| directive.value(key))
        .map(ToOwned::to_owned))
}

fn experimental_directive(directives: &[Directive], path: &Path, context: &str) -> Result<bool> {
    let Some(directive) = directive(directives, "experimental", path, context)? else {
        return Ok(false);
    };
    if !directive.args.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.experimental".to_string(),
            reason: "does not take arguments".to_string(),
        });
    }
    Ok(true)
}

fn delay_load_temporalio_workflow_directive(
    directives: &[Directive],
    path: &Path,
    context: &str,
) -> Result<bool> {
    let Some(directive) = directive(directives, "delay-load-temporalio-workflow", path, context)?
    else {
        return Ok(false);
    };
    if !directive.args.is_empty() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: "@nexus.delay-load-temporalio-workflow".to_string(),
            reason: "does not take arguments".to_string(),
        });
    }
    Ok(true)
}

fn directive<'a>(
    directives: &'a [Directive],
    name: &str,
    path: &Path,
    context: &str,
) -> Result<Option<&'a Directive>> {
    let mut matches = directives.iter().filter(|directive| directive.name == name);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: format!("@nexus.{name}"),
            reason: "duplicate directive".to_string(),
        });
    }
    Ok(first)
}

fn directive_language_value<'a>(directive: &'a Directive, language: Language) -> Option<&'a str> {
    directive.value(language_key(language))
}

fn all_languages() -> [Language; 6] {
    [
        Language::Dotnet,
        Language::Go,
        Language::Java,
        Language::Python,
        Language::Ruby,
        Language::TypeScript,
    ]
}

fn language_key(language: Language) -> &'static str {
    match language {
        Language::Dotnet => "dotnet",
        Language::Go => "go",
        Language::Java => "java",
        Language::Python => "python",
        Language::Ruby => "ruby",
        Language::TypeScript => "typescript",
    }
}

fn parse_directives(docs: Option<&str>, path: &Path, context: &str) -> Result<Vec<Directive>> {
    let Some(docs) = docs else {
        return Ok(Vec::new());
    };

    let mut directives = Vec::new();
    let mut current = None::<String>;

    for line in docs.lines() {
        let trimmed_start = line.trim_start();
        if trimmed_start.starts_with("@nexus.") {
            if let Some(previous) = current.take() {
                directives.push(parse_directive_line(&previous, path, context)?);
            }
            current = Some(trimmed_start.to_string());
            continue;
        }

        let is_continuation = current.is_some()
            && !trimmed_start.is_empty()
            && trimmed_start.len() != line.len()
            && (trimmed_start.starts_with('"') || trimmed_start.contains('='));

        if is_continuation {
            let directive = current
                .as_mut()
                .expect("continuation checked to have an active directive");
            directive.push(' ');
            directive.push_str(trimmed_start);
            continue;
        }

        if let Some(previous) = current.take() {
            directives.push(parse_directive_line(&previous, path, context)?);
        }
    }

    if let Some(previous) = current.take() {
        directives.push(parse_directive_line(&previous, path, context)?);
    }

    Ok(directives)
}

#[derive(Debug, Clone)]
struct Directive {
    name: String,
    args: BTreeMap<String, String>,
}

impl Directive {
    fn value(&self, key: &str) -> Option<&str> {
        self.args.get(key).map(String::as_str)
    }
}

fn parse_directive_line(line: &str, path: &Path, context: &str) -> Result<Directive> {
    let Some(rest) = line.strip_prefix("@nexus.") else {
        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: line.to_string(),
            reason: "directive must start with `@nexus.`".to_string(),
        });
    };

    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..name_end];
    let mut tail = rest[name_end..].trim_start();
    let mut args = BTreeMap::new();

    if tail.starts_with('"') {
        let (value, remaining) = parse_directive_value(tail, path, context, name)?;
        args.insert("value".to_string(), value);
        tail = remaining.trim_start();
    }

    while !tail.is_empty() {
        let key_end = tail
            .find(|character: char| character == '=' || character.is_whitespace())
            .unwrap_or(tail.len());
        let key = &tail[..key_end];
        let after_key = tail[key_end..].trim_start();
        let Some(after_equals) = after_key.strip_prefix('=') else {
            return Err(Error::InvalidWitDirective {
                path: path.to_path_buf(),
                context: context.to_string(),
                directive: format!("@nexus.{name}"),
                reason: format!("expected `=` after `{key}`"),
            });
        };
        let (value, remaining) =
            parse_directive_value(after_equals.trim_start(), path, context, name)?;
        args.insert(key.to_string(), value);
        tail = remaining.trim_start();
    }

    Ok(Directive {
        name: name.to_string(),
        args,
    })
}

fn parse_directive_value<'a>(
    input: &'a str,
    path: &Path,
    context: &str,
    name: &str,
) -> Result<(String, &'a str)> {
    if let Some(stripped) = input.strip_prefix('"') {
        let mut escaped = false;
        let mut value = String::new();
        for (index, character) in stripped.char_indices() {
            if escaped {
                value.push(character);
                escaped = false;
                continue;
            }
            match character {
                '\\' => escaped = true,
                '"' => return Ok((value, &stripped[index + 1..])),
                _ => value.push(character),
            }
        }

        return Err(Error::InvalidWitDirective {
            path: path.to_path_buf(),
            context: context.to_string(),
            directive: format!("@nexus.{name}"),
            reason: "unterminated quoted string".to_string(),
        });
    }

    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    Ok((input[..end].to_string(), &input[end..]))
}

fn parse_bool(value: &str) -> std::result::Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(format!("expected `true` or `false`, found `{value}`")),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::descriptors::DescriptorIndex;
    use crate::error::Error;
    use crate::language::Language;

    use super::{
        ApiSpec, AuthoredFieldTypeSpec, FunctionArgSpec, FunctionArgsSpec, directive,
        load_linked_wit_metadata_from_inputs, parse_directives,
    };

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn descriptors() -> DescriptorIndex {
        DescriptorIndex::load(&root().join("examples/descriptors/temporal_api.bin")).unwrap()
    }

    fn linked_inputs_path() -> PathBuf {
        root().join("examples/inputs/deps")
    }

    fn parse(language: Language, wit: &str) -> ApiSpec {
        ApiSpec::parse_for_language_with_inputs(
            language,
            wit,
            PathBuf::from("inline.wit"),
            &[linked_inputs_path()],
        )
        .unwrap()
    }

    fn validate(language: Language, wit: &str) -> Result<(), Error> {
        let spec = parse(language, wit);
        let descriptors = descriptors();
        crate::validation::validate_type_overrides(&spec, &descriptors, language)
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("nex-gen-{label}-{unique}"))
    }

    #[test]
    fn parses_enum_field_defaults() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{workflow-id-reuse-policy};

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.proto-field "workflow_id_reuse_policy"
    /// @nexus.default "allow-duplicate"
    id-reuse-policy: workflow-id-reuse-policy,
  }
}
"#;

        let spec = parse(Language::Python, wit);
        let request = spec
            .types
            .get("temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest")
            .unwrap();
        assert!(!request.is_field_required("workflow_id_reuse_policy"));
        assert_eq!(
            request
                .generated_model
                .field_default("workflow_id_reuse_policy")
                .map(|default| default.enum_value),
            Some(0)
        );
    }

    #[test]
    fn rejects_unknown_enum_field_default() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{workflow-id-reuse-policy};

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.proto-field "workflow_id_reuse_policy"
    /// @nexus.default "missing-case"
    id-reuse-policy: workflow-id-reuse-policy,
  }
}
"#;

        let error = ApiSpec::parse_for_language_with_inputs(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
            &[linked_inputs_path()],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unknown enum case `missing-case`")
        );
    }

    #[test]
    fn rejects_non_enum_field_default() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.proto-field "request_id"
    /// @nexus.default "request-id"
    request-id: string,
  }
}
"#;

        let error = ApiSpec::parse_for_language_with_inputs(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
            &[linked_inputs_path()],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("only enum field defaults are supported")
        );
    }

    #[test]
    fn rejects_enum_field_default_with_source() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{workflow-id-reuse-policy};

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.proto-field "workflow_id_reuse_policy"
    /// @nexus.default "allow-duplicate"
    /// @nexus.source "workflow_id_reuse_policy"
    id-reuse-policy: workflow-id-reuse-policy,
  }
}
"#;

        let error = ApiSpec::parse_for_language_with_inputs(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
            &[linked_inputs_path()],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot be combined with `@nexus.source`")
        );
    }

    #[test]
    fn parses_wit_into_selected_language_spec() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
/// @nexus.service-name "temporal.api.workflowservice.v1.WorkflowService"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{placeholder, retry-policy, signal-function, workflow-function};

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record signal-with-start-workflow-request {
    /// @nexus.proto-field "workflow_type"
    workflow: workflow-function,
    workflow-id: string,
    task-queue: string,
    /// @nexus.proto-field "signal_name"
    signal: signal-function,
    /// @nexus.source "workflow_namespace"
    namespace: option<string>,
    /// @nexus.omit
    header: placeholder,
    /// @nexus.omit
    time-skipping-config: placeholder,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse"
  record signal-with-start-workflow-response {
    run-id: option<string>,
    started: option<bool>,
    /// @nexus.omit
    signal-link: placeholder,
  }

  /// @nexus.output-transform
  ///   python-type="temporalio.workflow.ExternalWorkflowHandle[WorkflowResult]"
  ///   python="temporalio.workflow.get_external_workflow_handle(request.workflow_id, run_id=result.run_id)"
  ///   typescript-type="workflow.ExternalWorkflowHandle"
  ///   typescript="workflow.getExternalWorkflowHandle(request.workflowId, result.runId ?? undefined)"
  ///   typescript-package="@temporalio/workflow"
  signal-with-start-workflow-execution: func(
    request: signal-with-start-workflow-request
  ) -> signal-with-start-workflow-response;
}
"#;

        let python = parse(Language::Python, wit);
        let typescript = parse(Language::TypeScript, wit);
        let dotnet = parse(Language::Dotnet, wit);
        assert_eq!(python.services[0].name, "WorkflowService");
        assert_eq!(
            python.services[0].wire_name,
            "temporal.api.workflowservice.v1.WorkflowService"
        );
        assert_eq!(
            typescript.services[0].wire_name,
            "temporal.api.workflowservice.v1.WorkflowService"
        );
        assert!(
            python
                .imports_for_language(Language::Python)
                .iter()
                .any(|import| import.reference == "temporalio.workflow"
                    && import.module == "temporalio.workflow")
        );
        assert!(
            python
                .imports_for_language(Language::TypeScript)
                .iter()
                .any(|import| import.reference == "workflow"
                    && import.module == "@temporalio/workflow")
        );

        let python_support = python.support.fragments_for_language(Language::Python);
        let typescript_support = typescript
            .support
            .fragments_for_language(Language::TypeScript);
        let dotnet_support = dotnet.support.fragments_for_language(Language::Dotnet);
        assert_eq!(python_support.len(), 1);
        assert_eq!(typescript_support.len(), 1);
        assert_eq!(dotnet_support.len(), 1);
        assert!(
            python_support[0]
                .path
                .ends_with("deps/nexus-temporal-types/python/temporal_model_converters.py")
        );
        assert!(
            typescript_support[0]
                .path
                .ends_with("deps/nexus-temporal-types/typescript/temporal_model_converters.ts")
        );
        assert!(
            python_support[0]
                .contents
                .contains("def retry_policy_from_proto(")
        );
        assert!(
            typescript_support[0]
                .contents
                .contains("export function retryPolicyFromProto(")
        );
        assert_eq!(dotnet_support[0].prefix.as_deref(), Some("NexGen.Support"));
        assert!(
            python
                .type_override("temporal.api.common.v1.Payloads")
                .unwrap()
                .replacement
                .is_none()
        );

        let request = python
            .type_override(
                "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest",
            )
            .unwrap();
        assert_eq!(
            python.services[0].operations[0].input_proto(),
            Some("temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest")
        );
        assert_eq!(
            python.services[0].operations[0].output_proto(),
            Some("temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse")
        );
        assert!(request.is_field_required("workflow_type"));
        assert!(request.is_field_hidden("header"));
        assert!(request.is_field_omitted("header"));
        let model = request.generated_model().unwrap();
        assert_eq!(model.field_name_override("workflow_type"), Some("workflow"));
        assert_eq!(model.field_name_override("input"), Some("args"));
        assert_eq!(
            model.field_name_override("workflow_id"),
            Some("workflow-id")
        );
        assert!(model.function("workflow_type").unwrap().primary);
        assert_eq!(model.function("workflow_type").unwrap().converter, None);
        assert_eq!(model.function("workflow_type").unwrap().args_field, "input");
        assert_eq!(
            model
                .function("workflow_type")
                .unwrap()
                .result_type_parameter
                .as_deref(),
            Some("WorkflowResult")
        );
        assert_eq!(
            model
                .function("workflow_type")
                .unwrap()
                .alternate_type
                .as_ref()
                .unwrap()
                .to_wit_string(),
            "model.workflow-type"
        );
        assert_eq!(
            model.function("signal_name").unwrap().converter.as_deref(),
            Some("signal_function_to_proto")
        );
        assert_eq!(
            model
                .function("signal_name")
                .unwrap()
                .alternate_type
                .as_ref()
                .unwrap()
                .to_wit_string(),
            "string"
        );
        assert_eq!(
            model.field_source("namespace"),
            Some("workflow_namespace()")
        );

        let typescript_model = typescript
            .type_override(
                "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest",
            )
            .unwrap()
            .generated_model()
            .unwrap();
        assert!(
            python
                .type_override("temporal.api.sdk.v1.UserMetadata")
                .unwrap()
                .flatten_in_api()
        );
        assert!(typescript_model.function("workflow_type").is_some());
        assert!(typescript_model.with_arguments("signal_name").is_some());
        assert!(typescript_model.function("signal_name").is_some());
        assert_eq!(
            typescript_model
                .function("workflow_type")
                .unwrap()
                .converter,
            None
        );
        assert_eq!(
            typescript_model
                .function("signal_name")
                .unwrap()
                .converter
                .as_deref(),
            Some("signalFunctionToProto")
        );
    }

    #[test]
    fn rejects_legacy_with_arguments_directive_name() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.with-arguments
    ///   args-field="signal-input"
    ///   value-type="workflow.SignalDefinition<any[]>"
    ///   args-type="Value extends workflow.SignalDefinition<infer Args, any> ? Args : never"
    ///   name-expr="value.name"
    signal: string,
  }

  request-op: func(request: request) -> request;
}
"#;

        let error =
            ApiSpec::parse_for_language(Language::TypeScript, wit, PathBuf::from("inline.wit"))
                .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("renamed to `@nexus.typescript-with-arguments`")
        );
    }

    #[test]
    fn accepts_language_specific_source_helpers() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.source python="workflow_namespace" typescript="workflowNamespace" dotnet="TemporalWorkflowContext.WorkflowNamespace"
    namespace: option<string>,
  }

  request-op: func(request: request) -> request;
}
"#;

        let python =
            ApiSpec::parse_for_language(Language::Python, wit, PathBuf::from("inline.wit"))
                .unwrap();
        let typescript =
            ApiSpec::parse_for_language(Language::TypeScript, wit, PathBuf::from("inline.wit"))
                .unwrap();
        let dotnet =
            ApiSpec::parse_for_language(Language::Dotnet, wit, PathBuf::from("inline.wit"))
                .unwrap();
        assert_eq!(
            python
                .type_override(
                    "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
                )
                .unwrap()
                .field_source("namespace"),
            Some("workflow_namespace()")
        );
        assert_eq!(
            typescript
                .type_override(
                    "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
                )
                .unwrap()
                .field_source("namespace"),
            Some("workflowNamespace()")
        );
        assert_eq!(
            dotnet
                .type_override(
                    "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
                )
                .unwrap()
                .field_source("namespace"),
            Some("TemporalWorkflowContext.WorkflowNamespace()")
        );
    }

    #[test]
    fn parses_language_specific_api_docs() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.doc "Default field doc" python="Python field doc" typescript="TypeScript field doc"
    id: string,
  }

  /// @nexus.doc
  ///   "Default operation doc"
  ///   python="Python operation doc"
  ///   typescript="TypeScript operation doc"
  ///   returns="Default return doc"
  ///   python-returns="Python return doc"
  ///   typescript-returns="TypeScript return doc"
  request-op: func(request: request) -> request;
}
"#;

        let python = parse(Language::Python, wit);
        let typescript = parse(Language::TypeScript, wit);
        assert_eq!(
            python.services[0]
                .operation("RequestOp")
                .unwrap()
                .doc
                .for_language(Language::Python),
            Some("Python operation doc")
        );
        assert_eq!(
            python.services[0]
                .operation("RequestOp")
                .unwrap()
                .return_doc
                .for_language(Language::Python),
            Some("Python return doc")
        );
        assert_eq!(
            typescript.services[0]
                .operation("RequestOp")
                .unwrap()
                .doc
                .for_language(Language::TypeScript),
            Some("TypeScript operation doc")
        );
        assert_eq!(
            typescript.services[0]
                .operation("RequestOp")
                .unwrap()
                .return_doc
                .for_language(Language::TypeScript),
            Some("TypeScript return doc")
        );
        assert_eq!(
            python
                .type_override(
                    "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
                )
                .unwrap()
                .generated_model()
                .unwrap()
                .field_doc("id")
                .and_then(|doc| doc.for_language(Language::Python)),
            Some("Python field doc")
        );
    }

    #[test]
    fn parses_experimental_annotations() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
/// @nexus.experimental
interface workflow-service {
  /// @nexus.experimental
  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    id: string,
  }

  /// @nexus.experimental
  request-op: func(request: request) -> request;
}
"#;

        let spec = parse(Language::Python, wit);
        assert!(spec.services[0].experimental);
        assert!(
            spec.services[0]
                .operation("RequestOp")
                .unwrap()
                .experimental
        );
        assert!(
            spec.type_override(
                "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
            )
            .unwrap()
            .experimental()
        );
        assert!(
            spec.records
                .get("workflow-service.request")
                .unwrap()
                .experimental
        );
    }

    #[test]
    fn parses_delay_load_temporalio_workflow_annotation() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
/// @nexus.delay-load-temporalio-workflow
interface workflow-service {
  record request {
    id: string,
  }

  request-op: func(request: request) -> request;
}
"#;

        let spec = parse(Language::Python, wit);
        assert!(spec.services[0].delay_load_temporalio_workflow);
    }

    #[test]
    fn rejects_experimental_annotation_arguments() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
/// @nexus.experimental reason="preview"
interface workflow-service {
  record request {
    id: string,
  }

  request-op: func(request: request) -> request;
}
"#;

        let error = ApiSpec::parse_for_language(Language::Python, wit, PathBuf::from("inline.wit"))
            .unwrap_err();
        assert!(error.to_string().contains("@nexus.experimental"));
        assert!(error.to_string().contains("does not take arguments"));
    }

    #[test]
    fn infers_python_sequence_annotation_for_wit_lists() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  resource started-workflow {
    get-result: func() -> list<string>;
  }
}
"#;

        let python = parse(Language::Python, wit);
        let resource = python.services[0].resource("started-workflow").unwrap();
        let method = resource
            .methods
            .iter()
            .find(|method| method.name == "get-result")
            .unwrap();
        assert_eq!(
            method.result.as_ref().unwrap().result_type.to_wit_string(),
            "list<string>"
        );
    }

    #[test]
    fn records_fixed_function_signature_args_as_fields() {
        let wit = r#"
package temporal:function-execution@1.0.0;

world system {
  export function-execution;
}

interface functions {
  function-call: func(name: string, enabled: bool) -> string;

  /// @nexus.function
  ///   primary=true
  ///   signature="function-call"
  type executable-function = string;
}

/// @nexus.endpoint "function-execution"
interface function-execution {
  use functions.{executable-function};

  record execute-function-request {
    function: executable-function,
  }

  record execute-function-result {
    value: string,
  }

  execute-function: func(request: execute-function-request) -> execute-function-result;
}
"#;

        let spec = parse(Language::Python, wit);
        let request = spec
            .records
            .get("function-execution.execute-function-request")
            .unwrap();
        let model = &request.generated_model;
        assert_eq!(model.field_name_override("name"), Some("name"));
        assert_eq!(model.field_name_override("enabled"), Some("enabled"));
        assert_eq!(
            model.field_wit_type("name").unwrap().to_wit_string(),
            "string"
        );
        assert_eq!(
            model.field_wit_type("enabled").unwrap().to_wit_string(),
            "bool"
        );
        assert_eq!(
            model.function("function").unwrap().arg_fields,
            vec!["name".to_string(), "enabled".to_string()]
        );
        assert_ne!(
            model.field_wit_type("name").unwrap().to_wit_string(),
            "list<string>"
        );
        assert!(model.function("function").unwrap().primary);
        assert_eq!(model.function("function").unwrap().args_field, "args");
        assert_eq!(
            model.function("function").unwrap().args,
            FunctionArgsSpec::Fixed(vec![
                FunctionArgSpec {
                    name: "name".to_string(),
                    field_type: AuthoredFieldTypeSpec::String,
                },
                FunctionArgSpec {
                    name: "enabled".to_string(),
                    field_type: AuthoredFieldTypeSpec::Bool,
                },
            ])
        );
    }

    #[test]
    fn records_varargs_function_args_from_signature_annotation() {
        let wit = r#"
package temporal:function-execution@1.0.0;

world system {
  export function-execution;
}

interface functions {
  type function-args = list<string>;

  /// @nexus.function-args varargs=true
  function-call: func(args: function-args) -> string;

  /// @nexus.function
  ///   primary=true
  ///   signature="function-call"
  ///   args-field="args"
  type executable-function = string;
}

/// @nexus.endpoint "function-execution"
interface function-execution {
  use functions.{executable-function};

  record execute-function-request {
    function: executable-function,
  }

  record execute-function-result {
    value: string,
  }

  execute-function: func(request: execute-function-request) -> execute-function-result;
}
"#;

        let spec = parse(Language::Python, wit);
        let request = spec
            .records
            .get("function-execution.execute-function-request")
            .unwrap();
        let model = &request.generated_model;
        assert_eq!(
            model.field_wit_type("args").unwrap().to_wit_string(),
            "list<string>"
        );
        assert_eq!(
            model.function("function").unwrap().args,
            FunctionArgsSpec::Varargs {
                prefix: Vec::new(),
                typescript_drop_prefix: false,
            }
        );
    }

    #[test]
    fn validates_proto_backed_wit_field_types_and_keeps_flattened_types_separate() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{payload};

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record request {
    /// @nexus.proto-field "memo"
    /// @nexus.flattened-type python="str" typescript="string"
    metadata: option<payload>,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse"
  record response {
    run-id: option<string>,
  }

  run: func(request: request) -> response;
}
"#;

        let python = parse(Language::Python, wit);
        let python_model = python
            .type_override(
                "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest",
            )
            .unwrap()
            .generated_model()
            .unwrap();
        assert_eq!(
            python_model.field_wit_type("memo").unwrap().to_wit_string(),
            "option<model.payload>"
        );
        assert_eq!(python_model.field_annotation("memo"), None);
        assert_eq!(
            python_model
                .field_flattened_annotation("memo")
                .and_then(|annotation| annotation.for_language(Language::Python)),
            Some("str")
        );

        let mismatch = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{placeholder, retry-policy, task-queue};

  /// @nexus.proto "temporal.api.activity.v1.ActivityOptions"
  record request {
    task-queue: option<task-queue>,
    /// @nexus.proto-field "retry_policy"
    retry-policy: option<string>,
    /// @nexus.omit
    schedule-to-close-timeout: placeholder,
    /// @nexus.omit
    schedule-to-start-timeout: placeholder,
    /// @nexus.omit
    start-to-close-timeout: placeholder,
    /// @nexus.omit
    heartbeat-timeout: placeholder,
    /// @nexus.omit
    priority: placeholder,
  }

  run: func(request: request) -> request;
}
"#;

        let error = validate(Language::Python, mismatch).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("WIT field type `option<string>` does not match proto field type `option<temporal.api.common.v1.RetryPolicy>`")
        );
    }

    #[test]
    fn accumulates_linked_and_root_input_support_fragments() {
        let temp_dir = unique_temp_dir("support-fragments");
        fs::create_dir_all(&temp_dir).unwrap();
        let input_path = temp_dir.join("input.wit");
        let extra_support_path = temp_dir.join("extra_support.py");
        fs::write(
            &extra_support_path,
            "def extra_support_hook() -> str:\n    return 'extra'\n",
        )
        .unwrap();

        let wit = r#"
/// @nexus.support python="extra_support.py"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{retry-policy};

  retry-policy-operation: func(request: retry-policy) -> retry-policy;
}
"#;

        let spec = ApiSpec::parse_for_language_with_inputs(
            Language::Python,
            wit,
            input_path,
            &[linked_inputs_path()],
        )
        .unwrap();
        let python_support = spec.support.fragments_for_language(Language::Python);
        assert_eq!(python_support.len(), 2);
        assert!(
            python_support[0]
                .path
                .ends_with("deps/nexus-temporal-types/python/temporal_model_converters.py")
        );
        assert!(python_support[1].path.ends_with("extra_support.py"));
        assert!(
            python_support[0]
                .contents
                .contains("def retry_policy_from_proto(")
        );
        assert!(
            python_support[1]
                .contents
                .contains("def extra_support_hook() -> str:")
        );

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn parses_sibling_wit_files_from_main_wit_package_directory() {
        let temp_dir = unique_temp_dir("sibling-wit");
        fs::create_dir_all(&temp_dir).unwrap();
        let shared_path = temp_dir.join("shared.wit");
        let input_path = temp_dir.join("main.wit");
        fs::write(
            &shared_path,
            r#"
package temporal:nexus@1.0.0;

interface shared {
  /// @nexus.proto "acme.foo.v1.LocalRetryPolicy"
  record local-retry-policy {
  }
}
"#,
        )
        .unwrap();

        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  use shared.{local-retry-policy};

  retry-policy-operation: func(request: local-retry-policy) -> local-retry-policy;
}
"#;

        let spec = ApiSpec::parse_for_language_with_inputs(
            Language::Python,
            wit,
            input_path,
            &[linked_inputs_path()],
        )
        .unwrap();
        assert_eq!(
            spec.services[0].operations[0].input_proto(),
            Some("acme.foo.v1.LocalRetryPolicy")
        );

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn ignores_sibling_wit_files_for_standalone_input_wit() {
        let temp_dir = unique_temp_dir("standalone-wit");
        fs::create_dir_all(&temp_dir).unwrap();
        let shared_path = temp_dir.join("shared.wit");
        let input_path = temp_dir.join("input.wit");
        fs::write(
            &shared_path,
            r#"
package temporal:nexus@1.0.0;

interface shared {
  /// @nexus.proto "acme.foo.v1.LocalRetryPolicy"
  record local-retry-policy {
  }
}
"#,
        )
        .unwrap();

        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{retry-policy};

  retry-policy-operation: func(request: retry-policy) -> retry-policy;
}
"#;

        let spec = ApiSpec::parse_for_language_with_inputs(
            Language::Python,
            wit,
            input_path,
            &[linked_inputs_path()],
        )
        .unwrap();
        assert_eq!(
            spec.services[0].operations[0].input_proto(),
            Some("temporal.api.common.v1.RetryPolicy")
        );

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn loads_linked_wit_metadata_from_temporal_type_input() {
        let linked_types = load_linked_wit_metadata_from_inputs(&[linked_inputs_path()]).unwrap();

        let payload = linked_types
            .proto_types
            .get("temporal.api.common.v1.Payload")
            .unwrap();
        assert_eq!(payload.wit_name, "payload");
        assert_eq!(payload.use_path, "nexus:temporal-types/model@1.0.0");

        let task_queue = linked_types
            .proto_types
            .get("temporal.api.taskqueue.v1.TaskQueue")
            .unwrap();
        assert_eq!(task_queue.wit_name, "task-queue");
        assert_eq!(task_queue.use_path, "nexus:temporal-types/model@1.0.0");

        assert_eq!(
            linked_types
                .type_use_paths
                .get("workflow-function")
                .map(String::as_str),
            Some("nexus:temporal-types/model@1.0.0")
        );
        assert_eq!(
            linked_types
                .type_use_paths
                .get("signal-function")
                .map(String::as_str),
            Some("nexus:temporal-types/model@1.0.0")
        );
    }

    #[test]
    fn validates_wit_function_fields() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{duration, placeholder, signal-function, task-queue, workflow-function};

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record signal-with-start-workflow-request {
    /// @nexus.proto-field "workflow_type"
    workflow: workflow-function,
    workflow-id: string,
    task-queue: task-queue,
    /// @nexus.proto-field "signal_name"
    signal: signal-function,
    /// @nexus.source "workflow_namespace"
    namespace: option<string>,
    /// @nexus.omit
    workflow-execution-timeout: placeholder,
    /// @nexus.omit
    workflow-run-timeout: placeholder,
    /// @nexus.omit
    workflow-task-timeout: placeholder,
    /// @nexus.omit
    identity: placeholder,
    /// @nexus.omit
    request-id: placeholder,
    /// @nexus.omit
    workflow-id-reuse-policy: placeholder,
    /// @nexus.omit
    workflow-id-conflict-policy: placeholder,
    /// @nexus.omit
    control: placeholder,
    /// @nexus.omit
    retry-policy: placeholder,
    /// @nexus.omit
    cron-schedule: placeholder,
    /// @nexus.omit
    memo: placeholder,
    /// @nexus.omit
    search-attributes: placeholder,
    /// @nexus.omit
    header: placeholder,
    workflow-start-delay: option<duration>,
    /// @nexus.omit
    user-metadata: placeholder,
    /// @nexus.omit
    links: placeholder,
    /// @nexus.omit
    versioning-override: placeholder,
    /// @nexus.omit
    priority: placeholder,
    /// @nexus.omit
    time-skipping-config: placeholder,
  }

  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse"
  record signal-with-start-workflow-response {
    run-id: option<string>,
    started: option<bool>,
    /// @nexus.omit
    signal-link: placeholder,
  }

  signal-with-start-workflow-execution: func(
    request: signal-with-start-workflow-request
  ) -> signal-with-start-workflow-response;
}
"#;

        validate(Language::Python, wit).unwrap();
    }

    #[test]
    fn requires_explicit_omit_for_proto_fields_left_out_of_records() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{placeholder, retry-policy, task-queue};

  /// @nexus.proto "temporal.api.activity.v1.ActivityOptions"
  record activity-options {
    task-queue: option<task-queue>,
    retry-policy: retry-policy,
  }

  activity-options-operation: func(request: activity-options) -> activity-options;
}
"#;

        let error = validate(Language::Python, wit).unwrap_err();
        assert!(error.to_string().contains(
            "must declare field `schedule_to_close_timeout` in WIT or add that field and mark it with `@nexus.omit`"
        ));
    }

    #[test]
    fn allows_explicitly_omitted_proto_fields() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{placeholder, retry-policy, task-queue};

  /// @nexus.proto "temporal.api.activity.v1.ActivityOptions"
  record activity-options {
    task-queue: option<task-queue>,
    retry-policy: retry-policy,
    /// @nexus.omit
    schedule-to-close-timeout: placeholder,
    /// @nexus.omit
    schedule-to-start-timeout: placeholder,
    /// @nexus.omit
    start-to-close-timeout: placeholder,
    /// @nexus.omit
    heartbeat-timeout: placeholder,
    /// @nexus.omit
    priority: placeholder,
  }

  activity-options-operation: func(request: activity-options) -> activity-options;
}
"#;

        validate(Language::Python, wit).unwrap();
    }

    #[test]
    fn rejects_type_level_omit_directive() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

/// @nexus.endpoint "__temporal_system"
interface workflow-service {
  use nexus:temporal-types/model@1.0.0.{retry-policy, task-queue};

  /// @nexus.proto "temporal.api.activity.v1.ActivityOptions"
  /// @nexus.omit
  record activity-options {
    task-queue: option<task-queue>,
    retry-policy: retry-policy,
  }

  activity-options-operation: func(request: activity-options) -> activity-options;
}
"#;

        let error = ApiSpec::parse_for_language_with_inputs(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
            &[linked_inputs_path()],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("type-level omit is no longer supported")
        );
    }

    #[test]
    fn wit_parse_errors_include_parser_diagnostics() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export example;
}

interface example {
  record request {
    include: string,
  }
}
"#;

        let error = ApiSpec::parse_for_language(Language::Python, wit, PathBuf::from("inline.wit"))
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("expected an identifier or string"));
        assert!(message.contains("found keyword `include`"));
        assert!(message.contains("include: string"));
    }

    #[test]
    fn parses_multiline_directive_arguments() {
        let directives = parse_directives(
            Some(
                r#"@nexus.type
  python="temporalio.common.RetryPolicy"
  typescript="common.RetryPolicy""#,
            ),
            &PathBuf::from("inline.wit"),
            "type `example`",
        )
        .unwrap();

        let directive = directive(
            &directives,
            "type",
            &PathBuf::from("inline.wit"),
            "type `example`",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            directive.value("python"),
            Some("temporalio.common.RetryPolicy")
        );
        assert_eq!(directive.value("typescript"), Some("common.RetryPolicy"));
    }

    #[test]
    fn infers_python_imports_from_qualified_type_paths() {
        assert_eq!(
            super::python_qualified_module_paths(
                "temporalio.common.RetryPolicy | datetime.timedelta | typing.Any",
            ),
            ["datetime".to_string(), "temporalio.common".to_string()]
                .into_iter()
                .collect()
        );
        assert_eq!(
            super::python_qualified_module_paths(
                "str | collections.abc.Callable[..., collections.abc.Awaitable[object]]",
            ),
            BTreeSet::new()
        );
    }

    #[test]
    fn rejects_duplicate_proto_field_mappings() {
        let wit = r#"
package temporal:nexus@1.0.0;

world system {
  export workflow-service;
}

interface workflow-service {
  /// @nexus.proto "temporal.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest"
  record signal-with-start-workflow-request {
    /// @nexus.proto-field "workflow_id"
    workflow-id: string,
    /// @nexus.proto-field "workflow_id"
    workflow-id-alias: string,
  }
}
"#;

        let err = ApiSpec::parse_for_language(Language::Python, wit, PathBuf::from("inline.wit"))
            .unwrap_err();
        assert!(matches!(err, Error::InvalidWit { .. }));
    }
}
