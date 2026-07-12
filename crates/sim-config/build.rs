use std::{env, fs, io, path::PathBuf};

const CONFIG_PATH: &str = "config/simulation.toml";

fn main() -> io::Result<()> {
    let manifest_dir = PathBuf::from(required_env("CARGO_MANIFEST_DIR")?);
    let source = manifest_dir.join("../..").join(CONFIG_PATH);
    let out_dir = PathBuf::from(required_env("OUT_DIR")?);
    let profile_dir = out_dir
        .ancestors()
        .nth(3)
        .ok_or_else(|| io::Error::other("unexpected Cargo OUT_DIR layout"))?;
    let destination = profile_dir.join(CONFIG_PATH);

    println!("cargo:rerun-if-changed={}", source.display());
    fs::create_dir_all(destination.parent().expect("config has a parent"))?;
    fs::copy(source, destination)?;
    Ok(())
}

fn required_env(name: &str) -> io::Result<String> {
    env::var(name).map_err(|error| io::Error::other(format!("missing {name}: {error}")))
}
