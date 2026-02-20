use std::env;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let config_path = format!("{}/cbindgen.toml", crate_dir);
    let bindings_path = format!("{}/bindings.h", crate_dir);
    println!("cargo:rerun-if-changed={}", config_path);

    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .with_config(cbindgen::Config::from_file(config_path).unwrap())
        .generate()
        .map_or_else(
            |error| match error {
                cbindgen::Error::ParseSyntaxError { .. } => {
                    eprintln!(
                        "cargo:warning=Failed to parse source files for cbindgen: {}",
                        error
                    );
                }
                e => panic!("Failed to generate bindings: {:?}", e),
            },
            |bindings| {
                bindings.write_to_file(&bindings_path);
            },
        );
}
