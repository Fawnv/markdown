//! Dev helper: render Markdown from stdin to an HTML fragment on stdout,
//! with exactly the same pipeline the app uses.
//!
//!     cargo run --example render < README.md > out.html
//!     cargo run --release --example render -- --bench 200 < README.md   (timing, like typing in the app)

#[allow(dead_code)]
#[path = "../src/render.rs"]
mod render;

use std::io::{Read, Write};
use std::time::Instant;

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("read stdin");
    let args: Vec<String> = std::env::args().collect();
    if let Some(n) = args.iter().position(|a| a == "--bench").and_then(|i| args.get(i + 1)).and_then(|n| n.parse::<usize>().ok()) {
        let t = Instant::now();
        render::render(&input);
        eprintln!("first render (loads syntaxes): {:.1} ms", t.elapsed().as_secs_f64() * 1e3);
        // Simulate typing: every iteration the text differs by one character.
        let t = Instant::now();
        let mut text = input.clone();
        for i in 0..n {
            text.insert(text.find('\n').unwrap_or(0), if i % 2 == 0 { 'a' } else { 'b' });
            render::render(&text);
        }
        eprintln!("{} renders, {:.2} ms each ({} bytes)", n, t.elapsed().as_secs_f64() * 1e3 / n as f64, input.len());
        return;
    }
    std::io::stdout().write_all(render::render(&input).as_bytes()).expect("write stdout");
}
