//! Dev helper: render Markdown from stdin to an HTML fragment on stdout,
//! with exactly the same pipeline the app uses.
//!
//!     cargo run --example render < README.md > out.html

#[allow(dead_code)]
#[path = "../src/render.rs"]
mod render;

use std::io::{Read, Write};

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("read stdin");
    std::io::stdout().write_all(render::render(&input).as_bytes()).expect("write stdout");
}
