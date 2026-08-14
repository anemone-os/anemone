use std::collections::BTreeSet;

use anyhow::{Context, bail, ensure};
use nemophila_wasm::{CompilationMode, Config, Engine, ExternType, Module, ValType};
use wasmparser::Parser;

use super::interface::{CoreImport, InterfaceContract};

pub struct CheckedArtifact {
    pub module: Module,
    pub custom_sections: BTreeSet<String>,
}

impl core::fmt::Debug for CheckedArtifact {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("CheckedArtifact")
            .field("custom_sections", &self.custom_sections)
            .finish_non_exhaustive()
    }
}

pub fn validate(bytes: &[u8], contract: &InterfaceContract) -> anyhow::Result<CheckedArtifact> {
    if Parser::is_component(bytes) {
        bail!("artifact envelope rejected a Component binary; expected a Core module")
    }
    ensure!(
        Parser::is_core_wasm(bytes),
        "artifact envelope rejected a non-Core-WebAssembly candidate"
    );

    let mut config = Config::default();
    config.compilation_mode(CompilationMode::Eager);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, bytes)
        .context("nemophila-wasm rejected malformed, invalid, or unsupported Core Wasm")?;

    ensure!(
        !module.has_start(),
        "artifact envelope rejected a Core Wasm start section"
    );
    contract.verify_embedded_metadata(bytes)?;
    validate_imports(&module, contract)?;
    validate_exports(&module, contract)?;
    let custom_sections = validate_custom_sections(&module)?;

    Ok(CheckedArtifact {
        module,
        custom_sections,
    })
}

fn validate_imports(module: &Module, contract: &InterfaceContract) -> anyhow::Result<()> {
    let expected = contract.expected_imports()?;
    let mut actual = BTreeSet::new();
    for import in module.imports() {
        ensure!(
            matches!(import.ty(), ExternType::Func(_)),
            "artifact imports non-function item '{}::{}'",
            import.module(),
            import.name()
        );
        actual.insert(CoreImport {
            module: import.module().to_string(),
            function: import.name().to_string(),
        });
    }
    ensure!(
        actual == expected,
        "artifact Host imports do not match canonical WIT: expected {expected:?}, found {actual:?}"
    );
    Ok(())
}

fn validate_exports(module: &Module, contract: &InterfaceContract) -> anyhow::Result<()> {
    let expected_functions = contract.expected_exports()?;
    let mut found_functions = BTreeSet::new();
    let mut memory_count = 0usize;

    for export in module.exports() {
        match export.ty() {
            ExternType::Func(_) if expected_functions.contains(export.name()) => {
                found_functions.insert(export.name().to_string());
            },
            ExternType::Func(_)
                if matches!(
                    export.name(),
                    "cabi_realloc" | "cabi_realloc_wit_bindgen_0_51_0"
                ) => {},
            ExternType::Global(global)
                if matches!(export.name(), "__data_end" | "__heap_base")
                    && global.content() == ValType::I32
                    && global.mutability().is_const() => {},
            ExternType::Memory(_) if export.name() == "memory" => memory_count += 1,
            kind => bail!(
                "artifact exports unknown or extra item '{}' with type {kind:?}",
                export.name()
            ),
        }
    }

    ensure!(
        found_functions == expected_functions,
        "artifact lifecycle/callback exports do not match canonical WIT: expected {expected_functions:?}, found {found_functions:?}"
    );
    ensure!(
        memory_count == 1,
        "artifact must export exactly one guest linear memory"
    );
    Ok(())
}

fn validate_custom_sections(module: &Module) -> anyhow::Result<BTreeSet<String>> {
    let mut sections = BTreeSet::new();
    let mut component_types = 0usize;
    for section in module.custom_sections() {
        let name = section.name();
        if name.starts_with("component-type") {
            component_types += 1;
        } else if !matches!(name, "name" | "producers" | "target_features") {
            bail!("artifact contains unknown custom section '{name}'")
        }
        sections.insert(name.to_string());
    }
    ensure!(
        component_types > 0,
        "artifact has no WIT component-type custom section"
    );
    Ok(sections)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> InterfaceContract {
        InterfaceContract::load_from_path_for_test(&InterfaceContract::canonical_path())
    }

    fn core_module(body: &str, embed_metadata: bool) -> Vec<u8> {
        let mut wasm = wat::parse_str(format!("(module {body})")).unwrap();
        if embed_metadata {
            contract().embed_metadata(&mut wasm).unwrap();
        }
        wasm
    }

    fn valid_shape(extra_imports: &str, extra_definitions: &str) -> Vec<u8> {
        let contract = contract();
        core_module(
            &format!(
                r#"
                (import "{}" "{}" (func (result i32)))
                (import "{}" "{}" (func (param i32 i32 i32)))
                {extra_imports}
                (memory (export "memory") 1)
                (func (export "{}") (result i32) i32.const 0)
                (func (export "{}") (param i32 i32))
                {extra_definitions}
                "#,
                contract.register_import.module,
                contract.register_import.function,
                contract.log_import.module,
                contract.log_import.function,
                contract.load_export,
                contract.callback_export,
            ),
            true,
        )
    }

    #[test]
    fn canonical_core_module_shape_is_accepted() {
        validate(&valid_shape("", ""), &contract()).unwrap();
    }

    #[test]
    fn component_start_unknown_import_and_extra_export_are_envelope_failures() {
        let component = b"\0asm\x0d\0\x01\0";
        let error = validate(component, &contract()).unwrap_err().to_string();
        assert!(error.contains("Component binary"), "{error}");

        let start = valid_shape("", "(func $start) (start $start)");
        let error = validate(&start, &contract()).unwrap_err().to_string();
        assert!(error.contains("start section"), "{error}");

        let unknown_import = valid_shape("(import \"unknown\" \"service\" (func))", "");
        let error = validate(&unknown_import, &contract())
            .unwrap_err()
            .to_string();
        assert!(error.contains("Host imports"), "{error}");

        let extra_export = valid_shape("", "(func (export \"second-lifecycle-entry\"))");
        let error = validate(&extra_export, &contract())
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown or extra"), "{error}");
    }

    #[test]
    fn malformed_core_is_rejected_by_interpreter_owner() {
        let malformed = b"\0asm\x01\0\0\0\xff";
        let error = validate(malformed, &contract()).unwrap_err().to_string();
        assert!(error.contains("nemophila-wasm rejected"), "{error}");
    }

    #[test]
    fn missing_or_mismatched_wit_metadata_is_rejected() {
        let missing = core_module(
            "(memory (export \"memory\") 1) (func (export \"load\") (result i32) i32.const 0) (func (export \"observe-clone\") (param i32 i32))",
            false,
        );
        let error = validate(&missing, &contract()).unwrap_err().to_string();
        assert!(error.contains("component-type metadata"), "{error}");
    }
}
