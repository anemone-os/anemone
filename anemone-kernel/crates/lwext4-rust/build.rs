use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    let c_path = PathBuf::from("c/lwext4")
        .canonicalize()
        .expect("cannot canonicalize path");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let lwext4_lib = format!("lwext4-{arch}");
    let toolchain = Toolchain::detect(&target, &arch);

    let mut make = Command::new("make");
    make.args([
        "musl-generic",
        "-C",
        c_path.to_str().expect("invalid path of lwext4"),
    ])
    .arg(format!("ARCH={arch}"))
    .arg(format!(
        "ULIBC={}",
        if env::var("CARGO_FEATURE_STD").is_ok() {
            "OFF"
        } else {
            "ON"
        }
    ))
    .arg(format!("OUT_DIR={}", out_dir.display()));
    toolchain.apply_to_make(&mut make, &arch);

    let status = make
        .status()
        .expect("failed to execute process: make lwext4");
    assert!(status.success());

    generate_bindings_to_rust(&toolchain, &out_dir);

    println!("cargo:rustc-link-lib=static={lwext4_lib}");
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rerun-if-changed=c/wrapper.h");
    println!(
        "cargo:rerun-if-changed={}/Makefile",
        c_path.to_str().unwrap()
    );
    println!("cargo:rerun-if-changed={}/src", c_path.to_str().unwrap());
    println!(
        "cargo:rerun-if-changed={}/toolchain/musl-generic.cmake",
        c_path.to_str().unwrap()
    );
}

fn generate_bindings_to_rust(toolchain: &Toolchain, out_dir: &Path) {
    let mut builder = bindgen::Builder::default()
        .use_core()
        .wrap_unsafe_ops(true)
        .header("c/wrapper.h")
        .clang_arg("-I./c/lwext4/include")
        .clang_arg(format!(
            "-I{}/build_musl-generic/include/",
            out_dir.display()
        ))
        .layout_tests(false)
        .parse_callbacks(Box::new(CustomCargoCallbacks));

    for arg in toolchain.bindgen_clang_args() {
        builder = builder.clang_arg(arg);
    }

    let bindings = builder.generate().expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

struct Toolchain {
    cc: String,
    cxx: String,
    ar: String,
    sysroot: String,
    clang_target: String,
}

impl Toolchain {
    fn detect(target: &str, arch: &str) -> Self {
        let normalized_target = normalize_target(target);
        let normalized_arch = arch.to_ascii_uppercase();
        let clang_target = clang_target_for(arch);
        let toolchain_root = env_value([
            format!("LWEXT4_TOOLCHAIN_{normalized_arch}"),
            format!("LWEXT4_TOOLCHAIN_{normalized_target}"),
            "LWEXT4_TOOLCHAIN".to_string(),
        ]);
        let cc = resolve_compiler_path(
            toolchain_root.as_deref(),
            arch,
            [
                format!("LWEXT4_CC_{normalized_arch}"),
                format!("CC_{normalized_target}"),
                "LWEXT4_CC".to_string(),
                "CC".to_string(),
            ],
            &["cc", "gcc"],
        )
        .unwrap_or_else(|| {
            panic!(
                "missing musl C compiler for {arch}; set LWEXT4_TOOLCHAIN_{normalized_arch} or LWEXT4_CC_{normalized_arch}"
            )
        });
        let cxx = resolve_compiler_path(
            toolchain_root.as_deref(),
            arch,
            [
                format!("LWEXT4_CXX_{normalized_arch}"),
                format!("CXX_{normalized_target}"),
                "LWEXT4_CXX".to_string(),
                "CXX".to_string(),
            ],
            &["c++", "g++"],
        )
        .unwrap_or_else(|| default_compiler_name(arch, "c++"));
        let ar = resolve_compiler_path(
            toolchain_root.as_deref(),
            arch,
            [
                format!("LWEXT4_AR_{normalized_arch}"),
                format!("AR_{normalized_target}"),
                "LWEXT4_AR".to_string(),
                "AR".to_string(),
            ],
            &["ar"],
        )
        .unwrap_or_else(|| {
            panic!("missing musl archiver for {arch}; set LWEXT4_AR_{normalized_arch}")
        });
        let sysroot = env_value([
            format!("LWEXT4_SYSROOT_{normalized_arch}"),
            format!("LWEXT4_SYSROOT_{normalized_target}"),
            "LWEXT4_SYSROOT".to_string(),
        ])
        .or_else(|| toolchain_root.as_ref().and_then(|root| infer_sysroot(root, arch)))
        .or_else(|| probe_sysroot(&cc))
        .unwrap_or_else(|| {
            panic!(
                "missing musl sysroot for {arch}; set LWEXT4_SYSROOT_{normalized_arch} or LWEXT4_TOOLCHAIN_{normalized_arch}"
            )
        });

        Self {
            cc,
            cxx,
            ar,
            sysroot,
            clang_target,
        }
    }

    fn apply_to_make(&self, command: &mut Command, arch: &str) {
        command.env("ARCH", arch);
        command.env("CC", &self.cc);
        command.env("CXX", &self.cxx);
        command.env("AR", &self.ar);
        command.env("LWEXT4_SYSROOT", &self.sysroot);
    }

    fn bindgen_clang_args(&self) -> Vec<String> {
        vec![
            format!("--target={}", self.clang_target),
            format!("--sysroot={}", self.sysroot),
        ]
    }
}

fn normalize_target(target: &str) -> String {
    target
        .chars()
        .map(|ch| match ch {
            '-' | '.' => '_',
            other => other.to_ascii_uppercase(),
        })
        .collect()
}

fn env_value<const N: usize>(keys: [String; N]) -> Option<String> {
    for key in keys {
        println!("cargo:rerun-if-env-changed={key}");
        if let Ok(value) = env::var(&key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn resolve_compiler_path<const N: usize>(
    toolchain_root: Option<&str>,
    arch: &str,
    env_keys: [String; N],
    tool_suffixes: &[&str],
) -> Option<String> {
    if let Some(path) = env_value(env_keys) {
        return Some(path);
    }

    if let Some(root) = toolchain_root {
        if let Some(path) = resolve_tool_in_root(root, arch, tool_suffixes) {
            return Some(path);
        }
    }

    for suffix in tool_suffixes {
        let candidate = default_compiler_name(arch, suffix);
        if command_exists(&candidate) {
            return Some(candidate);
        }
    }

    None
}

fn resolve_tool_in_root(
    toolchain_root: &str,
    arch: &str,
    tool_suffixes: &[&str],
) -> Option<String> {
    let bin_dir = Path::new(toolchain_root).join("bin");
    let prefix = format!("{arch}-");

    for suffix in tool_suffixes {
        let suffix = format!("-{suffix}");
        let mut matches = bin_dir
            .read_dir()
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(&suffix))
            })
            .collect::<Vec<_>>();
        matches.sort();

        if let Some(path) = matches.into_iter().next() {
            return Some(path.display().to_string());
        }
    }

    None
}

fn infer_sysroot(toolchain_root: &str, arch: &str) -> Option<String> {
    let sysroot = Path::new(toolchain_root).join(format!("{arch}-linux-musl"));
    if sysroot.exists() {
        Some(sysroot.display().to_string())
    } else {
        None
    }
}

fn probe_sysroot(cc: &str) -> Option<String> {
    let output = Command::new(cc).arg("-print-sysroot").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let sysroot = String::from_utf8(output.stdout).ok()?;
    let sysroot = sysroot.trim();
    if sysroot.is_empty() {
        None
    } else {
        Some(sysroot.to_string())
    }
}

fn default_compiler_name(arch: &str, suffix: &str) -> String {
    format!("{arch}-linux-musl-{suffix}")
}

fn command_exists(program: &str) -> bool {
    Command::new("which")
        .arg(program)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn clang_target_for(arch: &str) -> String {
    match arch {
        "riscv64" => "riscv64-unknown-linux-musl".to_string(),
        "loongarch64" => "loongarch64-unknown-linux-musl".to_string(),
        other => format!("{other}-unknown-linux-musl"),
    }
}

#[derive(Debug)]
struct CustomCargoCallbacks;
impl bindgen::callbacks::ParseCallbacks for CustomCargoCallbacks {
    fn header_file(&self, filename: &str) {
        add_include(filename);
    }

    fn include_file(&self, filename: &str) {
        add_include(filename);
    }

    fn read_env_var(&self, key: &str) {
        println!("cargo:rerun-if-env-changed={key}");
    }
}

fn add_include(filename: &str) {
    if !Path::new(filename).ends_with("ext4_config.h") {
        println!("cargo:rerun-if-changed={filename}");
    }
}
