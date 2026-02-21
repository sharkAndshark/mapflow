use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

fn collect_files(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir)
        .map_err(|err| format!("failed to read directory {}: {}", dir.display(), err))?
    {
        let entry = entry.map_err(|err| format!("failed to read directory entry: {}", err))?;
        let path = entry.path();

        if path.is_dir() {
            collect_files(&path, root, files)?;
            continue;
        }

        let rel_path = path
            .strip_prefix(root)
            .map_err(|err| format!("failed to strip path prefix: {}", err))?
            .to_path_buf();
        files.push(rel_path);
    }
    Ok(())
}

fn write_stub(out_file: &Path) -> Result<(), String> {
    fs::write(
        out_file,
        "pub static EMBEDDED_WEB_ASSETS: &[(&str, &[u8])] = &[];\n",
    )
    .map_err(|err| format!("failed to write stub asset index: {}", err))
}

fn write_embedded_assets(out_file: &Path, dist_dir: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_files(dist_dir, dist_dir, &mut files)?;
    files.sort();

    let mut output = fs::File::create(out_file)
        .map_err(|err| format!("failed to create generated asset index: {}", err))?;
    writeln!(
        output,
        "pub static EMBEDDED_WEB_ASSETS: &[(&str, &[u8])] = &["
    )
    .map_err(|err| format!("failed to write asset index header: {}", err))?;

    for rel_path in files {
        let rel_string = rel_path.to_string_lossy().replace('\\', "/");
        let abs_path = dist_dir.join(&rel_path);
        let abs_string = abs_path.to_string_lossy().replace('\\', "/");

        println!("cargo:rerun-if-changed={}", abs_string);
        writeln!(
            output,
            "    ({rel_string:?}, include_bytes!({abs_string:?})),"
        )
        .map_err(|err| format!("failed to write asset entry: {}", err))?;
    }

    writeln!(output, "];").map_err(|err| format!("failed to write asset index footer: {}", err))?;
    Ok(())
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR should be set by Cargo"));
    let out_file = out_dir.join("embedded_web_dist.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_EMBED_WEB_DIST");

    if env::var_os("CARGO_FEATURE_EMBED_WEB_DIST").is_none() {
        write_stub(&out_file).expect("failed to write embedded web dist stub");
        return;
    }

    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set by Cargo"),
    );
    let dist_dir = manifest_dir.join("../frontend/dist");
    println!("cargo:rerun-if-changed={}", dist_dir.display());

    if !dist_dir.is_dir() {
        panic!(
            "embed-web-dist feature requires frontend build output at {}",
            dist_dir.display()
        );
    }

    write_embedded_assets(&out_file, &dist_dir).expect("failed to generate embedded web assets");
}
