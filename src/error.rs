use std::io;
use std::path::PathBuf;
use std::process::ExitStatus;

use thiserror::Error;

use crate::language::Language;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("failed to read `{path}`: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("failed to write `{path}`: {source}")]
    WriteFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("generated file path `{path}` is invalid: {reason}")]
    InvalidGeneratedPath { path: PathBuf, reason: String },

    #[error("generated file path `{path}` conflicts with another generated file")]
    GeneratedFileConflict { path: PathBuf },

    #[error(
        "flattened API for `{type_name}` would generate duplicate parameter `{field}` from `{conflicting_field}`"
    )]
    FlattenedApiFieldConflict {
        type_name: String,
        field: String,
        conflicting_field: String,
    },

    #[error("refusing to overwrite existing path `{path}`")]
    OutputPathExists { path: PathBuf },

    #[error("failed to run formatter `{command}` for `{path}`: {source}")]
    RunFormatter {
        path: PathBuf,
        command: String,
        #[source]
        source: io::Error,
    },

    #[error("formatter `{command}` failed for `{path}` with status {status}")]
    FormatterFailed {
        path: PathBuf,
        command: String,
        status: ExitStatus,
    },

    #[error("failed to run command `{command}` in `{cwd}`: {source}")]
    RunCommand {
        cwd: PathBuf,
        command: String,
        #[source]
        source: io::Error,
    },

    #[error("command `{command}` failed in `{cwd}` with status {status}")]
    CommandFailed {
        cwd: PathBuf,
        command: String,
        status: ExitStatus,
    },

    #[error("failed to parse WIT from `{path}`: {message}")]
    WitParse { path: PathBuf, message: String },

    #[error("invalid WIT in `{path}`: {reason}")]
    InvalidWit { path: PathBuf, reason: String },

    #[error("failed to parse JSON schema from `{path}`: {message}")]
    JsonSchemaParse { path: PathBuf, message: String },

    #[error("invalid JSON schema in `{path}`: {reason}")]
    InvalidJsonSchema { path: PathBuf, reason: String },

    #[error("unsupported input format for `{path}`; expected `.wit`, `.json`, `.yaml`, or `.yml`")]
    UnsupportedInputFormat { path: PathBuf },

    #[error(
        "mixed input formats are not supported: first input is `{first}`, but `{path}` is `{found}`"
    )]
    MixedInputFormats {
        first: &'static str,
        path: PathBuf,
        found: &'static str,
    },

    #[error("invalid WIT directive `{directive}` on {context} in `{path}`: {reason}")]
    InvalidWitDirective {
        path: PathBuf,
        context: String,
        directive: String,
        reason: String,
    },

    #[error("failed to decode descriptor set from `{path}`: {source}")]
    DescriptorDecode {
        path: PathBuf,
        #[source]
        source: prost::DecodeError,
    },

    #[error("duplicate descriptor {kind} `{name}`")]
    DuplicateDescriptorDefinition { kind: &'static str, name: String },

    #[error("language `{language}` is not implemented yet")]
    UnsupportedLanguage { language: Language },

    #[error("{language} support namespace `{namespace}` is not supported")]
    UnsupportedSupportNamespace {
        language: Language,
        namespace: String,
    },

    #[error("invalid {language} support namespace `{namespace}`: {reason}")]
    InvalidSupportNamespace {
        language: Language,
        namespace: String,
        reason: String,
    },

    #[error("RPC `{name}` was not found in the descriptor set")]
    UnknownRpcName { name: String },

    #[error("RPC name `{name}` is ambiguous; matches: {matches:?}")]
    AmbiguousRpcName { name: String, matches: Vec<String> },

    #[error("unknown {language} example `{example_id}`")]
    UnknownExampleId {
        language: Language,
        example_id: String,
    },

    #[error("cannot generate add-rpc WIT for `{context}`: {reason}")]
    UnsupportedAddRpc { context: String, reason: String },

    #[error("service `{service}` is missing an endpoint")]
    MissingServiceEndpoint { service: String },

    #[error("resource `{service}.{resource}` is invalid: {reason}")]
    InvalidResource {
        service: String,
        resource: String,
        reason: String,
    },

    #[error("resource method `{service}.{resource}.{method}` is invalid: {reason}")]
    InvalidResourceMethod {
        service: String,
        resource: String,
        method: String,
        reason: String,
    },

    #[error(
        "service `{service}` operation `{operation}` output is missing required `type` or `transform` field"
    )]
    IncompleteOperationOutputTransform { service: String, operation: String },

    #[error(
        "service `{service}` operation `{operation}` references unknown input proto type `{type_name}`"
    )]
    UnknownOperationInputProto {
        service: String,
        operation: String,
        type_name: String,
    },

    #[error(
        "service `{service}` operation `{operation}` references unknown output proto type `{type_name}`"
    )]
    UnknownOperationOutputProto {
        service: String,
        operation: String,
        type_name: String,
    },

    #[error(
        "type override `{type_name}` is missing required `type` field; `fromProto` and `toProto` default when omitted"
    )]
    IncompleteTypeOverride { type_name: String },

    #[error("type override references unknown proto type `{type_name}`")]
    UnknownTypeOverride { type_name: String },

    #[error("type override for `{message}` references unknown field `{field}`")]
    UnknownTypeOverrideField { message: String, field: String },

    #[error("type override for `{message}.{field}` cannot be both required and omitted")]
    ConflictingTypeOverrideField { message: String, field: String },

    #[error("type override for `{message}.{field}` cannot be both omitted and customized")]
    OmittedCustomizedTypeOverrideField { message: String, field: String },

    #[error(
        "type override for `{message}` must declare field `{field}` in WIT or add that field and mark it with `@nexus.omit`"
    )]
    UndeclaredTypeOverrideField { message: String, field: String },

    #[error(
        "type override for `{type_name}.{field}` is missing required field customization; expected one of `name`, `type`, `source`, or `function`"
    )]
    IncompleteTypeOverrideField { type_name: String, field: String },

    #[error(
        "type override for `{message}.{field}` cannot combine field `{property}` with `{conflicting_property}`"
    )]
    ConflictingTypeOverrideFieldProperties {
        message: String,
        field: String,
        property: &'static str,
        conflicting_property: &'static str,
    },

    #[error("type override for `{message}.{field}` cannot use field `{property}`")]
    UnsupportedTypeOverrideFieldProperty {
        message: String,
        field: String,
        property: &'static str,
    },

    #[error("type override for `{message}.{field}` has invalid field `{property}`: {reason}")]
    InvalidTypeOverrideField {
        message: String,
        field: String,
        property: &'static str,
        reason: String,
    },

    #[error("type override for `{message}.{field}` cannot be marked required: {reason}")]
    UnsupportedRequiredTypeField {
        message: String,
        field: String,
        reason: String,
    },

    #[error("type override for `{message}.{field}` cannot use field `source`: {reason}")]
    UnsupportedSourcedTypeField {
        message: String,
        field: String,
        reason: String,
    },

    #[error("type override for enum `{enumeration}` cannot use `{property}`")]
    UnsupportedEnumTypeOverrideProperty {
        enumeration: String,
        property: &'static str,
    },

    #[error("Go code generation does not support {context}: {reason}")]
    UnsupportedGoType { context: String, reason: String },

    #[error("Go proto conversion for {context} is not supported: {reason}")]
    UnsupportedGoProtoConversion { context: String, reason: String },

    #[error("type override `{type_name}` cannot use `{property}`")]
    UnsupportedTypeOverrideProperty {
        type_name: String,
        property: &'static str,
    },

    #[error(
        "type override `{type_name}` cannot combine `{property}` with `{conflicting_property}`"
    )]
    ConflictingTypeOverrideProperties {
        type_name: String,
        property: &'static str,
        conflicting_property: &'static str,
    },
}
