use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, bail, ensure};
use clap::{Args, Subcommand};
use serde::Deserialize;
use xshell::Shell;

use crate::log_progress;

const MANIFEST_PATH: &str = "xref/sources.toml";
const XREF_ROOT: &str = "xref";
const SUPPORTED_SCHEMA: u32 = 1;

#[derive(Args)]
#[command(arg_required_else_help = true)]
pub struct XrefArgs {
    #[command(subcommand)]
    command: XrefCommand,
}

#[derive(Subcommand)]
enum XrefCommand {
    #[command(about = "List registered reference sources")]
    List,
    #[command(about = "Fetch registered sources into xref/<id>")]
    Fetch(SourceSelection),
    #[command(about = "Check local reference source identity and cleanliness")]
    Check(SourceSelection),
}

#[derive(Args)]
struct SourceSelection {
    #[arg(value_name = "ID")]
    id: Option<String>,
    #[arg(long, conflicts_with = "id")]
    all: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    source: Vec<Source>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    scope: String,
    url: String,
    tag: Option<String>,
    commit: String,
}

pub fn run(args: XrefArgs) -> anyhow::Result<()> {
    let manifest = load_manifest(Path::new(MANIFEST_PATH))?;
    match args.command {
        XrefCommand::List => list(&manifest),
        XrefCommand::Fetch(selection) => {
            for source in selection.resolve(&manifest)? {
                fetch(source, Path::new(XREF_ROOT))?;
            }
            Ok(())
        },
        XrefCommand::Check(selection) => {
            for source in selection.resolve(&manifest)? {
                let path = Path::new(XREF_ROOT).join(&source.id);
                check_checkout(source, &path)?;
                log_progress!(
                    "XREF",
                    &format!("checked {} at {}", source.id, short_commit(source))
                );
            }
            Ok(())
        },
    }
}

impl SourceSelection {
    fn resolve<'a>(&self, manifest: &'a Manifest) -> anyhow::Result<Vec<&'a Source>> {
        if self.all {
            return Ok(manifest.source.iter().collect());
        }
        let id = self
            .id
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("provide a source ID or --all"))?;
        let source = manifest
            .source
            .iter()
            .find(|source| source.id == id)
            .ok_or_else(|| anyhow::anyhow!("unknown xref source `{id}`"))?;
        Ok(vec![source])
    }
}

fn load_manifest(path: &Path) -> anyhow::Result<Manifest> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read xref manifest {}", path.display()))?;
    parse_manifest(&contents)
        .with_context(|| format!("failed to load xref manifest {}", path.display()))
}

fn parse_manifest(contents: &str) -> anyhow::Result<Manifest> {
    let manifest: Manifest = toml::from_str(contents).context("invalid TOML")?;
    ensure!(
        manifest.schema == SUPPORTED_SCHEMA,
        "unsupported xref schema {}; expected {SUPPORTED_SCHEMA}",
        manifest.schema
    );
    ensure!(
        !manifest.source.is_empty(),
        "xref manifest must contain at least one source"
    );

    let mut ids = HashSet::new();
    for source in &manifest.source {
        ensure!(
            valid_id(&source.id),
            "invalid xref source ID `{}`",
            source.id
        );
        ensure!(
            ids.insert(source.id.as_str()),
            "duplicate xref source ID `{}`",
            source.id
        );
        ensure!(
            !source.scope.trim().is_empty() && !source.scope.contains(['\n', '\r']),
            "xref source `{}` has an invalid scope",
            source.id
        );
        ensure!(
            source.url.starts_with("https://") && !source.url.contains(['\n', '\r']),
            "xref source `{}` must use a canonical HTTPS URL",
            source.id
        );
        ensure!(
            valid_commit(&source.commit),
            "xref source `{}` commit must be 40 lowercase hexadecimal characters",
            source.id
        );
        if let Some(tag) = &source.tag {
            ensure!(
                !tag.is_empty()
                    && !tag.starts_with('-')
                    && !tag.chars().any(char::is_whitespace)
                    && !tag.contains(['~', '^', ':', '?', '*', '[', '\\']),
                "xref source `{}` has an invalid tag",
                source.id
            );
        }
    }
    Ok(manifest)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && !id.starts_with(['-', '.'])
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-._".contains(&byte)
        })
}

fn valid_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn list(manifest: &Manifest) -> anyhow::Result<()> {
    for source in &manifest.source {
        log_progress!(
            "XREF",
            &format!("{} {} - {}", source.id, short_commit(source), source.scope)
        );
    }
    Ok(())
}

fn short_commit(source: &Source) -> &str {
    &source.commit[..12]
}

fn fetch(source: &Source, root: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(root)
        .with_context(|| format!("failed to create xref directory {}", root.display()))?;
    let destination = root.join(&source.id);
    if destination.exists() {
        check_checkout(source, &destination).with_context(|| {
            format!(
                "refusing to modify existing xref checkout {}",
                destination.display()
            )
        })?;
        log_progress!(
            "XREF",
            &format!("already fetched {} at {}", source.id, short_commit(source))
        );
        return Ok(());
    }

    let temporary = temporary_checkout_path(root, &source.id)?;
    let action = (|| -> anyhow::Result<()> {
        clone_source(source, &temporary)?;
        check_checkout(source, &temporary)?;
        fs::rename(&temporary, &destination).with_context(|| {
            format!(
                "failed to publish xref checkout {} as {}",
                temporary.display(),
                destination.display()
            )
        })?;
        Ok(())
    })();
    if let Err(error) = action {
        return finish_with_cleanup(Err(error), remove_temporary_checkout(&temporary));
    }

    log_progress!(
        "XREF",
        &format!(
            "fetched {} at {} into {}",
            source.id,
            short_commit(source),
            destination.display()
        )
    );
    Ok(())
}

fn clone_source(source: &Source, destination: &Path) -> anyhow::Result<()> {
    let sh = Shell::new()?;
    let mut clone = sh.cmd("git").arg("clone").arg("--no-checkout");
    if let Some(tag) = &source.tag {
        clone = clone
            .arg("--depth=1")
            .arg("--single-branch")
            .arg("--branch")
            .arg(tag);
    }
    clone
        .arg("--")
        .arg(&source.url)
        .arg(destination)
        .run_echo()
        .with_context(|| format!("failed to clone xref source `{}`", source.id))?;

    if let Some(tag) = &source.tag {
        let tag_ref = format!("refs/tags/{tag}^{{commit}}");
        let actual = git_output(destination, ["rev-parse", "--verify", tag_ref.as_str()])?;
        ensure!(
            actual == source.commit,
            "xref source `{}` tag `{tag}` resolves to {actual}, expected {}",
            source.id,
            source.commit
        );
    }

    sh.cmd("git")
        .arg("-C")
        .arg(destination)
        .arg("checkout")
        .arg("--detach")
        .arg(&source.commit)
        .run_echo()
        .with_context(|| format!("failed to check out xref source `{}`", source.id))?;
    Ok(())
}

fn check_checkout(source: &Source, path: &Path) -> anyhow::Result<()> {
    ensure!(
        path.is_dir(),
        "xref checkout is not a directory: {}",
        path.display()
    );
    let inside = git_output(path, ["rev-parse", "--is-inside-work-tree"])?;
    ensure!(inside == "true", "{} is not a Git worktree", path.display());

    let origin = git_output(path, ["remote", "get-url", "origin"])?;
    ensure!(
        origin == source.url,
        "xref source `{}` origin is `{origin}`, expected `{}`",
        source.id,
        source.url
    );
    let head = git_output(path, ["rev-parse", "HEAD"])?;
    ensure!(
        head == source.commit,
        "xref source `{}` is at {head}, expected {}",
        source.id,
        source.commit
    );
    if let Some(tag) = &source.tag {
        let tag_ref = format!("refs/tags/{tag}^{{commit}}");
        let actual = git_output(path, ["rev-parse", "--verify", tag_ref.as_str()])?;
        ensure!(
            actual == source.commit,
            "xref source `{}` tag `{tag}` resolves to {actual}, expected {}",
            source.id,
            source.commit
        );
    }
    let status = git_output(path, ["status", "--porcelain=v1"])?;
    ensure!(
        status.is_empty(),
        "xref source `{}` has local modifications:\n{status}",
        source.id
    );
    Ok(())
}

fn temporary_checkout_path(root: &Path, id: &str) -> anyhow::Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX epoch")?
        .as_nanos();
    let path = root.join(format!(".{id}.fetch-{}-{timestamp}", std::process::id()));
    ensure!(
        !path.exists(),
        "temporary xref checkout already exists: {}",
        path.display()
    );
    Ok(path)
}

fn remove_temporary_checkout(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    fs::remove_dir_all(path).with_context(|| {
        format!(
            "failed to remove temporary xref checkout {}",
            path.display()
        )
    })
}

fn finish_with_cleanup<T>(
    action: anyhow::Result<T>,
    cleanup: anyhow::Result<()>,
) -> anyhow::Result<T> {
    match (action, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(action), Err(cleanup)) => {
            bail!("{action:#}; temporary checkout cleanup also failed: {cleanup:#}")
        },
    }
}

fn git_output<'a>(
    path: &Path,
    arguments: impl IntoIterator<Item = &'a str>,
) -> anyhow::Result<String> {
    Ok(Shell::new()?
        .cmd("git")
        .arg("-C")
        .arg(path)
        .args(arguments)
        .read()?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "anemone-xtask-xref-{}-{timestamp}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn test_manifest() -> &'static str {
        r#"
schema = 1

[[source]]
id = "example-1.0"
scope = "Test-only source."
url = "https://example.com/example.git"
tag = "v1.0"
commit = "0123456789abcdef0123456789abcdef01234567"
"#
    }

    fn run_git(path: &Path, arguments: &[&str]) -> String {
        git_output(path, arguments.iter().copied()).unwrap()
    }

    #[test]
    fn manifest_accepts_minimal_git_metadata() {
        let manifest = parse_manifest(test_manifest()).unwrap();
        assert_eq!(manifest.schema, 1);
        assert_eq!(manifest.source[0].id, "example-1.0");
    }

    #[test]
    fn manifest_rejects_duplicate_ids_and_short_commits() {
        let duplicate = format!("{}\n{}", test_manifest(), &test_manifest()[12..]);
        assert!(parse_manifest(&duplicate).is_err());
        assert!(
            parse_manifest(
                &test_manifest().replace("0123456789abcdef0123456789abcdef01234567", "01234567")
            )
            .is_err()
        );
    }

    #[test]
    fn fetch_is_idempotent_and_check_rejects_dirty_checkout() {
        let workspace = TestDirectory::new();
        let upstream = workspace.0.join("upstream");
        fs::create_dir(&upstream).unwrap();
        run_git(&upstream, &["init", "-q"]);
        fs::write(upstream.join("README"), "fixture\n").unwrap();
        run_git(&upstream, &["add", "README"]);
        run_git(
            &upstream,
            &[
                "-c",
                "user.name=Xref Test",
                "-c",
                "user.email=xref@example.com",
                "commit",
                "-q",
                "-m",
                "fixture",
            ],
        );
        run_git(&upstream, &["tag", "v1"]);
        let commit = run_git(&upstream, &["rev-parse", "HEAD"]);
        let source = Source {
            id: "fixture".to_string(),
            scope: "Test-only source.".to_string(),
            url: upstream.to_string_lossy().into_owned(),
            tag: Some("v1".to_string()),
            commit,
        };
        let root = workspace.0.join("xref");

        fetch(&source, &root).unwrap();
        fetch(&source, &root).unwrap();
        let checkout = root.join("fixture");
        check_checkout(&source, &checkout).unwrap();
        fs::write(checkout.join("dirty"), "dirty\n").unwrap();
        assert!(check_checkout(&source, &checkout).is_err());
        assert!(fetch(&source, &root).is_err());
    }
}
