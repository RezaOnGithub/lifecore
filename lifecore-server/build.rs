use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

const PLACEHOLDER: &str = "<!doctype html>
<html lang=\"en\">
  <head><meta charset=\"utf-8\" /><title>LifeCore</title></head>
  <body>
    <p>This lifecore binary was built with LIFECORE_UI=skip and embeds no client.
    Run <code>lifecore serve --ui-dir DIR</code> with a built client, or rebuild
    after <code>npm ci</code> in <code>lifecore-server/ui</code>.</p>
  </body>
</html>
";

// Bundles the client with the installed esbuild into OUT_DIR, where the
// server embeds `index.html` and `app.js`. It never installs packages; with
// LIFECORE_UI=skip it embeds a placeholder so builds need no Node.
fn main() {
    println!("cargo::rerun-if-env-changed=LIFECORE_UI");
    for input in [
        "ui/src",
        "ui/index.html",
        "ui/package-lock.json",
        "ui/package.json",
        "ui/build.mjs",
        "ui/tsconfig.json",
    ] {
        println!("cargo::rerun-if-changed={input}");
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let ui =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("ui");
    match env::var("LIFECORE_UI") {
    Ok(value) if value == "skip" => {
        write(&out.join("index.html"), PLACEHOLDER);
        write(&out.join("app.js"), "");
        return;
    }
    Ok(value) => {
        return fail(&format!(
            "LIFECORE_UI={value:?} is not supported; unset it, or set LIFECORE_UI=skip to embed a placeholder page"
        ));
    }
    Err(env::VarError::NotUnicode(_)) => {
        return fail("LIFECORE_UI is not valid Unicode; unset it or set LIFECORE_UI=skip");
    }
    Err(env::VarError::NotPresent) => {}
    }
    if !ui.join("node_modules").is_dir() {
        return fail(&format!(
            "{} is missing. Run `npm ci` in lifecore-server/ui to install the client toolchain, or set LIFECORE_UI=skip to embed a placeholder page",
            ui.join("node_modules").display()
        ));
    }
    let status = Command::new("node")
        .arg("build.mjs")
        .arg("--outdir")
        .arg(&out)
        .current_dir(&ui)
        .status();
    match status {
    Ok(status) if status.success() => {}
    Ok(status) => fail(&format!(
        "bundling the client with esbuild failed ({status})"
    )),
    Err(error) => fail(&format!(
        "cannot run `node` to bundle the client: {error}. Install Node, or set LIFECORE_UI=skip to embed a placeholder page"
    )),
    }
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

// Cargo fails the build once the script exits after reporting an error.
fn fail(message: &str) {
    println!("cargo::error={message}");
}
