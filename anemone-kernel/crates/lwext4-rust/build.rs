use std::{
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

const LOCAL_DOCKER_CONFIG: &str = "local_docker.toml";

fn main() {
    let executor = BuildExecutor::detect();
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .canonicalize()
        .expect("cannot canonicalize CARGO_MANIFEST_DIR");
    let c_path = PathBuf::from("c/lwext4")
        .canonicalize()
        .expect("cannot canonicalize path");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let lwext4_lib = format!("lwext4-{arch}");
    let toolchain = Toolchain::detect(&executor, &target, &arch);
    let command_c_path = executor.command_path(&c_path, &manifest_dir);
    let command_out_dir = executor.command_path(&out_dir, &manifest_dir);
    let make_env = toolchain.make_env(&arch);

    let mut make = executor.command("make", &make_env);
    make.args([
        "musl-generic",
        "-C",
        command_c_path
            .to_str()
            .expect("invalid command path of lwext4"),
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
    .arg(format!("OUT_DIR={}", command_out_dir.display()));

    let status = make
        .status()
        .expect("failed to execute process: make lwext4");
    assert!(status.success(), "failed to build lwext4");

    generate_bindings_to_rust(&executor, &toolchain, &out_dir);

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

fn generate_bindings_to_rust(executor: &BuildExecutor, toolchain: &Toolchain, out_dir: &Path) {
    let bindgen_sysroot = executor.bindgen_sysroot(&toolchain.sysroot, out_dir);
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

    for arg in toolchain.bindgen_clang_args(&bindgen_sysroot) {
        builder = builder.clang_arg(arg);
    }

    let bindings = builder.generate().expect("Unable to generate bindings");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}

enum BuildExecutor {
    Host,
    Docker(DockerContainer),
}

impl BuildExecutor {
    fn detect() -> Self {
        println!("cargo:rerun-if-changed={LOCAL_DOCKER_CONFIG}");
        if !Path::new(LOCAL_DOCKER_CONFIG).exists() {
            return Self::Host;
        }

        let contents = fs::read_to_string(LOCAL_DOCKER_CONFIG)
            .unwrap_or_else(|error| panic!("failed to read {LOCAL_DOCKER_CONFIG}: {error}"));
        let config = toml::from_str::<LocalDockerConfig>(&contents)
            .unwrap_or_else(|error| panic!("failed to parse {LOCAL_DOCKER_CONFIG}: {error}"));
        Self::Docker(DockerContainer::ensure_running(config.container))
    }

    fn command(&self, program: &str, envs: &[(&str, &str)]) -> Command {
        match self {
            Self::Host => {
                let mut command = Command::new(program);
                command.envs(envs.iter().copied());
                command
            },
            Self::Docker(container) => container.command(program, envs),
        }
    }

    fn command_path(&self, host_path: &Path, manifest_dir: &Path) -> PathBuf {
        match self {
            Self::Host => host_path.to_path_buf(),
            Self::Docker(container) => container
                .config
                .workdir
                .join(relative_path(host_path, manifest_dir)),
        }
    }

    fn env_value(&self, key: &str) -> Option<String> {
        let Self::Docker(_) = self else {
            return None;
        };
        let output = self.command("printenv", &[]).arg(key).output().ok()?;
        if !output.status.success() {
            return None;
        }

        nonempty_stdout(output.stdout)
    }

    fn entries_in(&self, directory: &Path) -> Option<Vec<PathBuf>> {
        match self {
            Self::Host => Some(
                directory
                    .read_dir()
                    .ok()?
                    .flatten()
                    .map(|entry| entry.path())
                    .collect(),
            ),
            Self::Docker(_) => {
                let output = self
                    .command("find", &[])
                    .arg(directory)
                    .args(["-mindepth", "1", "-maxdepth", "1", "-print"])
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }

                let stdout = String::from_utf8(output.stdout).ok()?;
                Some(stdout.lines().map(PathBuf::from).collect())
            },
        }
    }

    fn path_is_dir(&self, path: &Path) -> bool {
        match self {
            Self::Host => path.is_dir(),
            Self::Docker(_) => self
                .command("test", &[])
                .args(["-d"])
                .arg(path)
                .status()
                .map(|status| status.success())
                .unwrap_or(false),
        }
    }

    fn bindgen_sysroot(&self, sysroot: &str, out_dir: &Path) -> PathBuf {
        match self {
            Self::Host => PathBuf::from(sysroot),
            Self::Docker(container) => container.copy_sysroot_headers(sysroot, out_dir),
        }
    }
}

#[derive(Deserialize)]
struct LocalDockerConfig {
    container: DockerContainerConfig,
}

#[derive(Deserialize)]
struct DockerContainerConfig {
    name: String,
    user: String,
    workdir: PathBuf,
}

struct DockerContainer {
    config: DockerContainerConfig,
    // This is lifecycle ownership, not a cached running-state snapshot: only this build may stop
    // a container after it successfully changed that container from stopped to running.
    started_by_build: bool,
}

impl DockerContainer {
    fn ensure_running(config: DockerContainerConfig) -> Self {
        assert!(!config.name.trim().is_empty(), "container.name is empty");
        assert!(!config.user.trim().is_empty(), "container.user is empty");
        assert!(
            config.workdir.is_absolute(),
            "container.workdir must be an absolute path"
        );

        let inspect = Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}"])
            .arg(&config.name)
            .output()
            .unwrap_or_else(|error| panic!("failed to inspect Docker container: {error}"));
        assert!(
            inspect.status.success(),
            "failed to inspect Docker container {}: {}",
            config.name,
            String::from_utf8_lossy(&inspect.stderr).trim()
        );

        let running = String::from_utf8(inspect.stdout)
            .expect("docker inspect returned non-UTF-8 output")
            .trim()
            .parse::<bool>()
            .expect("docker inspect returned an invalid container running state");
        let mut container = Self {
            config,
            started_by_build: false,
        };
        if !running {
            let status = Command::new("docker")
                .arg("start")
                .arg(&container.config.name)
                .status()
                .unwrap_or_else(|error| panic!("failed to start Docker container: {error}"));
            assert!(
                status.success(),
                "failed to start Docker container {}",
                container.config.name
            );
            container.started_by_build = true;
        }

        container
    }

    fn command(&self, program: &str, envs: &[(&str, &str)]) -> Command {
        let mut command = Command::new("docker");
        command
            .arg("exec")
            .args(["--user", &self.config.user])
            .arg("--workdir")
            .arg(&self.config.workdir);
        for (key, value) in envs {
            command.args(["--env", &format!("{key}={value}")]);
        }
        command.arg(&self.config.name).arg(program);
        command
    }

    fn copy_sysroot_headers(&self, sysroot: &str, out_dir: &Path) -> PathBuf {
        let local_sysroot = out_dir.join("docker-sysroot");
        if local_sysroot.exists() {
            fs::remove_dir_all(&local_sysroot)
                .expect("failed to remove stale Docker sysroot headers");
        }
        let local_include = local_sysroot.join("include");
        fs::create_dir_all(&local_include).expect("failed to create Docker sysroot include path");

        let source = format!(
            "{}:{}/include/.",
            self.config.name,
            sysroot.trim_end_matches('/')
        );
        let status = Command::new("docker")
            .args(["cp", &source])
            .arg(&local_include)
            .status()
            .unwrap_or_else(|error| panic!("failed to copy Docker sysroot headers: {error}"));
        assert!(
            status.success(),
            "failed to copy sysroot headers from Docker container {}",
            self.config.name
        );
        local_sysroot
    }
}

impl Drop for DockerContainer {
    fn drop(&mut self) {
        if !self.started_by_build {
            return;
        }

        let result = Command::new("docker")
            .arg("stop")
            .arg(&self.config.name)
            .status();
        match result {
            Ok(status) if status.success() => {},
            Ok(status) => eprintln!(
                "failed to stop Docker container {}: {status}",
                self.config.name
            ),
            Err(error) => eprintln!(
                "failed to stop Docker container {}: {error}",
                self.config.name
            ),
        }
    }
}

fn relative_path(path: &Path, base: &Path) -> PathBuf {
    let path_components = path.components().collect::<Vec<_>>();
    let base_components = base.components().collect::<Vec<_>>();
    let common_len = path_components
        .iter()
        .zip(&base_components)
        .take_while(|(path, base)| path == base)
        .count();
    assert!(common_len > 0, "cannot map host path into Docker workdir");

    let mut relative = PathBuf::new();
    for _ in common_len..base_components.len() {
        relative.push("..");
    }
    for component in &path_components[common_len..] {
        relative.push(component.as_os_str());
    }
    relative
}

fn nonempty_stdout(stdout: Vec<u8>) -> Option<String> {
    let value = String::from_utf8(stdout).ok()?;
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

struct Toolchain {
    cc: String,
    cxx: String,
    ar: String,
    sysroot: String,
    clang_target: String,
}

impl Toolchain {
    fn detect(executor: &BuildExecutor, target: &str, arch: &str) -> Self {
        let normalized_target = normalize_target(target);
        let normalized_arch = arch.to_ascii_uppercase();
        let clang_target = clang_target_for(arch);
        let toolchain_root = env_value(executor, [
            format!("LWEXT4_TOOLCHAIN_{normalized_arch}"),
            format!("LWEXT4_TOOLCHAIN_{normalized_target}"),
            "LWEXT4_TOOLCHAIN".to_string(),
        ]);
        let cc = resolve_compiler_path(
            executor,
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
            executor,
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
            executor,
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
        let sysroot = env_value(executor, [
            format!("LWEXT4_SYSROOT_{normalized_arch}"),
            format!("LWEXT4_SYSROOT_{normalized_target}"),
            "LWEXT4_SYSROOT".to_string(),
        ])
        .or_else(|| {
            toolchain_root
                .as_ref()
                .and_then(|root| infer_sysroot(executor, root, arch))
        })
        .or_else(|| probe_sysroot(executor, &cc))
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

    fn make_env<'a>(&'a self, arch: &'a str) -> [(&'static str, &'a str); 5] {
        [
            ("ARCH", arch),
            ("CC", &self.cc),
            ("CXX", &self.cxx),
            ("AR", &self.ar),
            ("LWEXT4_SYSROOT", &self.sysroot),
        ]
    }

    fn bindgen_clang_args(&self, sysroot: &Path) -> Vec<String> {
        vec![
            format!("--target={}", self.clang_target),
            format!("--sysroot={}", sysroot.display()),
            format!("-isystem{}", sysroot.join("include").display()),
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

fn env_value<const N: usize>(executor: &BuildExecutor, keys: [String; N]) -> Option<String> {
    for key in keys {
        println!("cargo:rerun-if-env-changed={key}");
        if let Ok(value) = env::var(&key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(value) = executor.env_value(&key) {
            return Some(value);
        }
    }
    None
}

fn resolve_compiler_path<const N: usize>(
    executor: &BuildExecutor,
    toolchain_root: Option<&str>,
    arch: &str,
    env_keys: [String; N],
    tool_suffixes: &[&str],
) -> Option<String> {
    if let Some(path) = env_value(executor, env_keys) {
        return Some(path);
    }

    if let Some(root) = toolchain_root {
        if let Some(path) = resolve_tool_in_root(executor, root, arch, tool_suffixes) {
            return Some(path);
        }
    }

    for suffix in tool_suffixes {
        let candidate = default_compiler_name(arch, suffix);
        if command_exists(executor, &candidate) {
            return Some(candidate);
        }
    }

    None
}

fn resolve_tool_in_root(
    executor: &BuildExecutor,
    toolchain_root: &str,
    arch: &str,
    tool_suffixes: &[&str],
) -> Option<String> {
    let bin_dir = Path::new(toolchain_root).join("bin");
    let prefix = format!("{arch}-");

    for suffix in tool_suffixes {
        let suffix = format!("-{suffix}");
        let mut matches = executor
            .entries_in(&bin_dir)?
            .into_iter()
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

fn infer_sysroot(executor: &BuildExecutor, toolchain_root: &str, arch: &str) -> Option<String> {
    let sysroot = Path::new(toolchain_root).join(format!("{arch}-linux-musl"));
    if executor.path_is_dir(&sysroot) {
        Some(sysroot.display().to_string())
    } else {
        None
    }
}

fn probe_sysroot(executor: &BuildExecutor, cc: &str) -> Option<String> {
    let output = executor
        .command(cc, &[])
        .arg("-print-sysroot")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    nonempty_stdout(output.stdout)
}

fn default_compiler_name(arch: &str, suffix: &str) -> String {
    format!("{arch}-linux-musl-{suffix}")
}

fn command_exists(executor: &BuildExecutor, program: &str) -> bool {
    executor
        .command("which", &[])
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
