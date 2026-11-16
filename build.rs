//! Гарантирует наличие web/dist для include_dir! даже без сборки фронтенда.
//! В CI и в релизе dist собирается заранее (`npm --prefix web run build`).
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("web").join("dist");
    let index = dist.join("index.html");
    if !index.is_file() {
        let _ = std::fs::create_dir_all(&dist);
        let _ = std::fs::write(
            &index,
            "<!doctype html><html lang=\"ru\"><meta charset=\"utf-8\">\
             <title>wireproxy-control</title>\
             <body style=\"font-family:system-ui;padding:2rem\">\
             <h1>UI не собрана</h1>\
             <p>Выполните <code>npm --prefix web install &amp;&amp; npm --prefix web run build</code> \
             и пересоберите бинарник.</p></body></html>",
        );
    }
    println!("cargo:rerun-if-changed=web/dist");
    println!("cargo:rerun-if-changed=web/src");
}
