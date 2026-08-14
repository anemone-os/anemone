use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail, ensure};
use wit_component::{StringEncoding, metadata};
use wit_parser::{Function, InterfaceId, Resolve, Type, TypeDefKind, WorldId, WorldItem, WorldKey};

pub const WIT_PATH: &str = "nemophila/wit";
pub const WIT_WORLD: &str = "anemone:nemophila/module@0.1.0";

const WEAVE_INTERFACE: &str = "weave-clone";
const REGISTER_FUNCTION: &str = "register-observer";
const REGISTRATION_RESULT: &str = "registration-result";
const LOGGING_INTERFACE: &str = "logging";
const LOG_FUNCTION: &str = "write";
const LOG_LEVEL: &str = "level";
const LOAD_FUNCTION: &str = "load";
const CALLBACK_FUNCTION: &str = "observe-clone";

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct CoreImport {
    pub module: String,
    pub function: String,
}

pub struct InterfaceContract {
    resolve: Resolve,
    world: WorldId,
    pub identity: String,
    pub register_import: CoreImport,
    pub log_import: CoreImport,
    pub load_export: String,
    pub callback_export: String,
    pub registered: u32,
    pub provider_unavailable: u32,
    pub already_registered: u32,
    pub load_success: i32,
    pub load_error: i32,
    pub log_debug: u32,
    pub log_info: u32,
}

impl InterfaceContract {
    pub fn load(workspace_root: &Path) -> anyhow::Result<Self> {
        Self::load_from_path(&workspace_root.join(WIT_PATH))
    }

    fn load_from_path(path: &Path) -> anyhow::Result<Self> {
        let mut resolve = Resolve::default();
        let (package, _) = resolve
            .push_path(path)
            .with_context(|| format!("failed to parse canonical WIT at '{}'", path.display()))?;
        let world = resolve.select_world(&[package], Some(WIT_WORLD))?;
        let identity = resolve.id_of_name(package, &resolve.worlds[world].name);
        ensure!(
            identity == WIT_WORLD,
            "canonical WIT selected world '{identity}', expected '{WIT_WORLD}'"
        );

        let weave = imported_interface(&resolve, world, WEAVE_INTERFACE)?;
        let logging = imported_interface(&resolve, world, LOGGING_INTERFACE)?;
        let register = interface_function(&resolve, weave, REGISTER_FUNCTION)?;
        let log = interface_function(&resolve, logging, LOG_FUNCTION)?;
        validate_register_shape(&resolve, register, weave)?;
        validate_log_shape(&resolve, log, logging)?;

        let load = exported_function(&resolve, world, LOAD_FUNCTION)?;
        let callback = exported_function(&resolve, world, CALLBACK_FUNCTION)?;
        validate_load_shape(&resolve, load)?;
        validate_callback_shape(callback)?;

        let registration_cases = enum_cases(&resolve, weave, REGISTRATION_RESULT)?;
        let log_cases = enum_cases(&resolve, logging, LOG_LEVEL)?;

        Ok(Self {
            identity,
            register_import: CoreImport {
                module: interface_identity(&resolve, weave)?,
                function: register.name.clone(),
            },
            log_import: CoreImport {
                module: interface_identity(&resolve, logging)?,
                function: log.name.clone(),
            },
            load_export: load.name.clone(),
            callback_export: callback.name.clone(),
            registered: case_index(&registration_cases, "registered")?,
            provider_unavailable: case_index(&registration_cases, "provider-unavailable")?,
            already_registered: case_index(&registration_cases, "already-registered")?,
            // Canonical ABI lowers the two cases of `result` to these
            // discriminants. The validated WIT shape above remains the logical
            // source of whether this export is a unit result.
            load_success: 0,
            load_error: 1,
            log_debug: case_index(&log_cases, "debug")?,
            log_info: case_index(&log_cases, "info")?,
            resolve,
            world,
        })
    }

    #[cfg(test)]
    pub fn load_from_path_for_test(path: &Path) -> Self {
        Self::load_from_path(path).expect("canonical WIT must parse")
    }

    pub fn expected_imports(&self) -> anyhow::Result<BTreeSet<CoreImport>> {
        let mut imports = BTreeSet::new();
        for item in self.resolve.worlds[self.world].imports.values() {
            let id = match item {
                WorldItem::Interface { id, .. } => *id,
                // A world-level `use` aliases an interface-owned type but does
                // not create a Core Wasm Host import.
                WorldItem::Type(_) => continue,
                WorldItem::Function(_) => {
                    bail!("canonical WIT world contains a direct Host function import")
                },
            };
            let module = interface_identity(&self.resolve, id)?;
            for function in self.resolve.interfaces[id].functions.values() {
                imports.insert(CoreImport {
                    module: module.clone(),
                    function: function.name.clone(),
                });
            }
        }
        Ok(imports)
    }

    pub fn expected_exports(&self) -> anyhow::Result<BTreeSet<String>> {
        let mut exports = BTreeSet::new();
        for (key, item) in &self.resolve.worlds[self.world].exports {
            let (WorldKey::Name(name), WorldItem::Function(function)) = (key, item) else {
                bail!("canonical WIT world contains a non-function module export")
            };
            ensure!(
                name == &function.name,
                "WIT export name does not match function"
            );
            exports.insert(name.clone());
        }
        Ok(exports)
    }

    pub fn verify_embedded_metadata(&self, artifact: &[u8]) -> anyhow::Result<()> {
        let (stripped, actual) =
            metadata::decode(artifact).context("failed to decode WIT component-type metadata")?;
        ensure!(
            stripped.is_some(),
            "artifact has no WIT component-type metadata"
        );

        let expected = metadata::encode(&self.resolve, self.world, StringEncoding::UTF8, None)?;
        let matching_worlds = actual
            .resolve
            .worlds
            .iter()
            .filter_map(|(world, item)| {
                let package = item.package?;
                (actual.resolve.id_of_name(package, &item.name) == WIT_WORLD).then_some(world)
            })
            .collect::<Vec<_>>();
        ensure!(
            matching_worlds.len() == 1,
            "artifact metadata does not contain exactly one {WIT_WORLD} world"
        );
        let actual = metadata::encode(
            &actual.resolve,
            matching_worlds[0],
            StringEncoding::UTF8,
            None,
        )?;
        ensure!(
            actual == expected,
            "artifact WIT identity or interface shape does not match {WIT_WORLD}"
        );
        Ok(())
    }

    #[cfg(test)]
    pub fn canonical_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../nemophila/wit")
    }

    #[cfg(test)]
    pub fn embed_metadata(&self, wasm: &mut Vec<u8>) -> anyhow::Result<()> {
        wit_component::embed_component_metadata(
            wasm,
            &self.resolve,
            self.world,
            StringEncoding::UTF8,
        )?;
        Ok(())
    }
}

fn imported_interface(
    resolve: &Resolve,
    world: WorldId,
    expected_name: &str,
) -> anyhow::Result<InterfaceId> {
    resolve.worlds[world]
        .imports
        .values()
        .find_map(|item| match item {
            WorldItem::Interface { id, .. }
                if resolve.interfaces[*id].name.as_deref() == Some(expected_name) =>
            {
                Some(*id)
            },
            _ => None,
        })
        .with_context(|| format!("canonical WIT does not import interface '{expected_name}'"))
}

fn interface_identity(resolve: &Resolve, interface: InterfaceId) -> anyhow::Result<String> {
    let interface = &resolve.interfaces[interface];
    let package = interface.package.context("WIT interface has no package")?;
    let name = interface
        .name
        .as_deref()
        .context("WIT interface has no name")?;
    Ok(resolve.id_of_name(package, name))
}

fn interface_function<'a>(
    resolve: &'a Resolve,
    interface: InterfaceId,
    name: &str,
) -> anyhow::Result<&'a Function> {
    resolve.interfaces[interface]
        .functions
        .get(name)
        .with_context(|| format!("WIT interface has no function '{name}'"))
}

fn exported_function<'a>(
    resolve: &'a Resolve,
    world: WorldId,
    name: &str,
) -> anyhow::Result<&'a Function> {
    resolve.worlds[world]
        .exports
        .iter()
        .find_map(|(key, item)| match (key, item) {
            (WorldKey::Name(key), WorldItem::Function(function)) if key == name => Some(function),
            _ => None,
        })
        .with_context(|| format!("canonical WIT world has no export '{name}'"))
}

fn named_type(resolve: &Resolve, interface: InterfaceId, name: &str) -> anyhow::Result<Type> {
    resolve.interfaces[interface]
        .types
        .get(name)
        .copied()
        .map(Type::Id)
        .with_context(|| format!("WIT interface has no type '{name}'"))
}

fn enum_cases(
    resolve: &Resolve,
    interface: InterfaceId,
    name: &str,
) -> anyhow::Result<Vec<String>> {
    let Type::Id(id) = named_type(resolve, interface, name)? else {
        unreachable!()
    };
    let TypeDefKind::Enum(enumeration) = &resolve.types[id].kind else {
        bail!("WIT type '{name}' is not an enum")
    };
    Ok(enumeration
        .cases
        .iter()
        .map(|case| case.name.clone())
        .collect())
}

fn case_index(cases: &[String], name: &str) -> anyhow::Result<u32> {
    cases
        .iter()
        .position(|case| case == name)
        .map(|index| index as u32)
        .with_context(|| format!("WIT enum has no case '{name}'"))
}

fn validate_register_shape(
    resolve: &Resolve,
    function: &Function,
    weave: InterfaceId,
) -> anyhow::Result<()> {
    ensure!(
        function.params.is_empty(),
        "register-observer must take no raw callback value"
    );
    ensure!(
        function.result == Some(named_type(resolve, weave, REGISTRATION_RESULT)?),
        "register-observer must return registration-result"
    );
    Ok(())
}

fn validate_log_shape(
    resolve: &Resolve,
    function: &Function,
    logging: InterfaceId,
) -> anyhow::Result<()> {
    ensure!(
        function.params
            == vec![
                (
                    "level".to_string(),
                    named_type(resolve, logging, LOG_LEVEL)?
                ),
                ("message".to_string(), Type::String),
            ]
            && function.result.is_none(),
        "logging.write must be the value-only level/string boundary"
    );
    Ok(())
}

fn validate_load_shape(resolve: &Resolve, function: &Function) -> anyhow::Result<()> {
    ensure!(
        function.params.is_empty(),
        "module load must take no raw context"
    );
    ensure!(
        function
            .result
            .is_some_and(|result| unit_result(resolve, result)),
        "module load must return a unit result independent of registration state"
    );
    Ok(())
}

fn unit_result(resolve: &Resolve, mut ty: Type) -> bool {
    loop {
        match ty {
            Type::Id(id) => match resolve.types[id].kind {
                TypeDefKind::Type(next) => ty = next,
                TypeDefKind::Result(ref result) => {
                    return result.ok.is_none() && result.err.is_none();
                },
                _ => return false,
            },
            _ => return false,
        }
    }
}

fn validate_callback_shape(function: &Function) -> anyhow::Result<()> {
    ensure!(
        function.params
            == vec![
                ("creator-tid".to_string(), Type::U32),
                ("child-tid".to_string(), Type::U32),
            ]
            && function.result.is_none(),
        "observe-clone must accept exactly two u32 TID values"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_wit_drives_identity_imports_exports_and_values() {
        let contract = InterfaceContract::load_from_path(&InterfaceContract::canonical_path())
            .expect("canonical WIT must parse");
        assert_eq!(contract.identity, WIT_WORLD);
        assert_eq!(contract.expected_imports().unwrap().len(), 2);
        assert_eq!(
            contract.expected_exports().unwrap(),
            BTreeSet::from(["load".to_string(), "observe-clone".to_string()])
        );
        assert_ne!(contract.registered, contract.provider_unavailable);
        assert_ne!(contract.log_info, 0);
    }
}
