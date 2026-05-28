use std::{fs, io, path::Path};

fn main() -> io::Result<()> {
    rerun_if_changed(Path::new("ltp/profile.txt"));
    rerun_tree(Path::new("ltp/groups"))?;
    rerun_tree(Path::new("fixtures"))?;
    Ok(())
}

fn rerun_tree(path: &Path) -> io::Result<()> {
    rerun_if_changed(path);

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            rerun_tree(&path)?;
        } else {
            rerun_if_changed(&path);
        }
    }

    Ok(())
}

fn rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
}
