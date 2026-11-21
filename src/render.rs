//! Markdown -> HTML, GitHub flavored. Code blocks are highlighted with syntect
//! using class names (`hl-*`) so colors come from CSS tokens (light/dark).

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use comrak::adapters::SyntaxHighlighterAdapter;
use comrak::nodes::NodeValue;
use comrak::options::Plugins;
use comrak::{Arena, Options, format_html_with_plugins, parse_document};
use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Code blocks larger than this are shown without highlighting (keeps typing snappy).
const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;

static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();

thread_local! {
    /// (syntax name, code) -> highlighted HTML. Rendering runs on one worker thread.
    static HL_CACHE: std::cell::RefCell<HashMap<(String, String), String>> = std::cell::RefCell::new(HashMap::new());
}

fn syntaxes() -> &'static SyntaxSet {
    SYNTAXES.get_or_init(two_face::syntax::extra_newlines)
}

/// Warm the syntax set on a background thread so the first code block renders fast.
pub fn preload() {
    std::thread::spawn(|| {
        syntaxes();
    });
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    // GFM
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.autolink = true;
    o.extension.tasklist = true;
    o.extension.footnotes = true;
    o.extension.alerts = true;
    o.extension.shortcodes = true;
    o.extension.header_id_prefix = Some(String::new());
    o.extension.front_matter_delimiter = Some("---".to_string());
    // Rendering: raw HTML is allowed like on GitHub; the UI sanitizes it and a strict CSP blocks scripts.
    o.render.r#unsafe = true;
    o.render.sourcepos = true;
    o.render.tasklist_classes = true;
    o.render.gfm_quirks = true;
    o
}

pub fn render(markdown: &str) -> String {
    let opts = options();
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &opts);

    let mut out = String::with_capacity(markdown.len() * 2);
    for node in root.children() {
        if let NodeValue::FrontMatter(ref fm) = node.data().value {
            front_matter_table(fm, &mut out);
        }
    }

    let adapter = Highlighter;
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);
    if format_html_with_plugins(root, &opts, &mut out, &plugins).is_err() {
        out.clear();
        out.push_str("<pre>");
        escape_html(&mut out, markdown).ok();
        out.push_str("</pre>");
    }
    out
}

/// GitHub renders YAML front matter as a small table; do the same for flat `key: value` maps.
fn front_matter_table(raw: &str, out: &mut String) {
    let body: Vec<&str> = raw
        .lines()
        .map(str::trim_end)
        .filter(|l| l.trim() != "---" && !l.trim().is_empty())
        .collect();
    let mut rows: Vec<(String, String)> = Vec::new();
    for line in &body {
        if line.starts_with(char::is_whitespace) || line.starts_with('-') {
            // nested value / list item: append to previous row
            if let Some(last) = rows.last_mut() {
                if !last.1.is_empty() {
                    last.1.push('\n');
                }
                last.1.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            rows.push((k.trim().to_string(), v.trim().trim_matches('"').to_string()));
        }
    }
    if rows.is_empty() {
        return;
    }
    out.push_str("<table class=\"front-matter\"><thead><tr>");
    for (k, _) in &rows {
        out.push_str("<th>");
        escape_html(out, k).ok();
        out.push_str("</th>");
    }
    out.push_str("</tr></thead><tbody><tr>");
    for (_, v) in &rows {
        out.push_str("<td>");
        escape_html(out, v).ok();
        out.push_str("</td>");
    }
    out.push_str("</tr></tbody></table>\n");
}

struct Highlighter;

impl Highlighter {
    fn find(lang: &str) -> Option<&'static SyntaxReference> {
        let ss = syntaxes();
        let token = lang
            .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if token.is_empty() {
            return None;
        }
        let alias = match token.as_str() {
            "sh" | "shell" | "console" | "shellsession" | "zsh" | "bash" => "Bourne Again Shell (bash)",
            "ps" | "ps1" | "pwsh" | "powershell" => "PowerShell",
            "js" | "javascript" | "mjs" | "cjs" | "node" => "JavaScript",
            "jsx" => "JavaScript (Babel)",
            "ts" | "typescript" | "mts" | "cts" => "TypeScript",
            "tsx" => "TypeScriptReact",
            "py" | "python3" | "py3" => "Python",
            "rs" => "Rust",
            "cs" | "csharp" | "c#" => "C#",
            "c++" | "cpp" | "cxx" | "hpp" => "C++",
            "objc" | "objective-c" => "Objective-C",
            "golang" => "Go",
            "kt" | "kotlin" => "Kotlin",
            "yml" => "YAML",
            "jsonc" | "json5" => "JSON",
            "md" | "markdown" => "Markdown",
            "html" | "htm" | "xhtml" => "HTML",
            "xml" | "svg" | "xaml" | "csproj" => "XML",
            "dockerfile" | "docker" => "Dockerfile",
            "make" | "makefile" => "Makefile",
            "bat" | "cmd" | "batch" => "Batch File",
            "diff" | "patch" => "Diff",
            "ini" | "cfg" | "conf" | "editorconfig" => "INI",
            "tf" | "hcl" | "terraform" => "Terraform",
            "rb" => "Ruby",
            "hs" => "Haskell",
            "ex" | "exs" | "elixir" => "Elixir",
            "proto" | "protobuf" => "Protocol Buffer",
            "vue" => "Vue Component",
            "text" | "txt" | "plain" | "plaintext" | "output" => return None,
            _ => "",
        };
        if !alias.is_empty()
            && let Some(s) = ss.find_syntax_by_name(alias)
        {
            return Some(s);
        }
        ss.find_syntax_by_token(&token)
            .or_else(|| ss.syntaxes().iter().find(|s| s.name.eq_ignore_ascii_case(&token)))
    }
}

impl SyntaxHighlighterAdapter for Highlighter {
    fn write_highlighted(&self, output: &mut dyn fmt::Write, lang: Option<&str>, code: &str) -> fmt::Result {
        let syntax = match lang {
            Some(l) if code.len() <= MAX_HIGHLIGHT_BYTES => Self::find(l),
            _ => None,
        };
        let Some(syntax) = syntax else {
            return escape_html(output, code);
        };
        // While typing, usually only one block changes: reuse the others' highlighting.
        let key = (syntax.name.clone(), code.to_string());
        if let Some(hit) = HL_CACHE.with(|c| c.borrow().get(&key).cloned()) {
            return output.write_str(&hit);
        }
        let mut generator = ClassedHTMLGenerator::new_with_class_style(
            syntax,
            syntaxes(),
            ClassStyle::SpacedPrefixed { prefix: "hl-" },
        );
        for line in LinesWithEndings::from(code) {
            if generator.parse_html_for_line_which_includes_newline(line).is_err() {
                return escape_html(output, code);
            }
        }
        let html = generator.finalize();
        HL_CACHE.with(|c| {
            let mut c = c.borrow_mut();
            if c.len() >= 512 {
                c.clear();
            }
            c.insert(key, html.clone());
        });
        output.write_str(&html)
    }

    fn write_pre_tag(&self, output: &mut dyn fmt::Write, attributes: HashMap<&'static str, Cow<'_, str>>) -> fmt::Result {
        write_tag(output, "pre", attributes)
    }

    fn write_code_tag(&self, output: &mut dyn fmt::Write, attributes: HashMap<&'static str, Cow<'_, str>>) -> fmt::Result {
        write_tag(output, "code", attributes)
    }
}

fn write_tag(output: &mut dyn fmt::Write, tag: &str, attributes: HashMap<&'static str, Cow<'_, str>>) -> fmt::Result {
    output.write_char('<')?;
    output.write_str(tag)?;
    let mut attrs: Vec<_> = attributes.into_iter().collect();
    attrs.sort_by_key(|(k, _)| *k);
    for (k, v) in attrs {
        write!(output, " {k}=\"")?;
        escape_html(output, &v)?;
        output.write_char('"')?;
    }
    output.write_char('>')
}

fn escape_html(output: &mut dyn fmt::Write, s: &str) -> fmt::Result {
    let mut last = 0;
    for (i, b) in s.bytes().enumerate() {
        let rep = match b {
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'&' => "&amp;",
            b'"' => "&quot;",
            _ => continue,
        };
        output.write_str(&s[last..i])?;
        output.write_str(rep)?;
        last = i + 1;
    }
    output.write_str(&s[last..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfm_features() {
        let html = render(
            "# Title\n\n- [x] done\n- [ ] todo\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n\n~~old~~ :rocket: https://github.com\n\n> [!NOTE]\n> hi\n\n```rust\nfn main() {}\n```\n\nText[^1]\n\n[^1]: Footnote.\n",
        );
        assert!(html.contains("<h1"), "{html}");
        assert!(html.contains("id=\"title\""), "{html}");
        assert!(html.contains("task-list-item-checkbox"), "{html}");
        assert!(html.contains("<table"), "{html}");
        assert!(html.contains("<del"), "{html}");
        assert!(html.contains('🚀'), "{html}");
        assert!(html.contains("href=\"https://github.com\""), "{html}");
        assert!(html.contains("markdown-alert-note"), "{html}");
        assert!(html.contains("hl-storage") || html.contains("hl-keyword"), "{html}");
        assert!(html.contains("footnote"), "{html}");
        assert!(html.contains("data-sourcepos"), "{html}");
    }

    #[test]
    fn front_matter_becomes_table() {
        let html = render("---\ntitle: Hello\ntags:\n  - a\n---\n\n# Body\n");
        assert!(html.starts_with("<table class=\"front-matter\">"), "{html}");
        assert!(html.contains("<th>title</th>"), "{html}");
    }

    #[test]
    fn languages_resolve() {
        for l in ["rust", "ts", "tsx", "py", "toml", "yaml", "json", "bash", "powershell", "c#", "go", "dockerfile", "sql", "diff", "html", "css"] {
            assert!(Highlighter::find(l).is_some(), "no syntax for {l}");
        }
    }
}

