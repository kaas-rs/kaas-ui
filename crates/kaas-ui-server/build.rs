//! Guarantee that `web/dist` exists before `rust-embed` looks at it.
//!
//! The frontend is embedded at compile time, so the Rust build depends on a
//! directory produced by a different toolchain. In the container image the
//! frontend stage runs first and the directory is real. On a developer's
//! machine, and in the CI job that only runs `cargo xtask ci`, it may not
//! exist at all — and `rust-embed` fails the build rather than embedding
//! nothing.
//!
//! So: create it if missing, and put a page there that says exactly what
//! happened. A binary that serves "the frontend was not built into this
//! binary" is debuggable; one that fails to compile in CI for a reason that
//! has nothing to do with the Rust code is a daily tax.

use std::path::PathBuf;

fn main() {
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("web")
        .join("dist");

    println!("cargo:rerun-if-changed={}", dist.display());

    if let Err(error) = std::fs::create_dir_all(&dist) {
        println!("cargo:warning=could not create {}: {error}", dist.display());
        return;
    }

    let index = dist.join("index.html");
    if index.exists() {
        return;
    }

    let placeholder = r#"<!doctype html>
<meta charset="utf-8">
<title>kaas-ui — frontend not built</title>
<style>
  body { font: 14px/1.6 ui-sans-serif, system-ui, sans-serif; background: #E1E1DB;
         color: #262625; margin: 0; display: grid; place-items: center; min-height: 100vh; }
  main { max-width: 44rem; padding: 2rem; }
  code { font-family: ui-monospace, monospace; background: #DADAD3; padding: .1em .35em; }
  h1 { font-size: 1.25rem; }
</style>
<main>
  <h1>The frontend was not built into this binary.</h1>
  <p>
    <code>web/dist</code> was empty when this binary was compiled, so there is
    nothing to serve. The API is unaffected — try
    <code>/api/clusters</code> or <code>/health</code>.
  </p>
  <p>To build it: <code>cd web &amp;&amp; npm ci &amp;&amp; npm run build</code>, then rebuild.</p>
</main>
"#;

    if let Err(error) = std::fs::write(&index, placeholder) {
        println!("cargo:warning=could not write {}: {error}", index.display());
    }
}
