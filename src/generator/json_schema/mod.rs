pub(crate) mod dotnet;
pub(crate) mod go;
pub(crate) mod java;
pub(crate) mod python;
pub(crate) mod typescript;

pub(in crate::generator) use crate::planning::build_json_name_manifest;
