use std::sync::LazyLock;

use regex::Regex;

static RE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[code\](.*?)\[/code\]").unwrap());
static RE_BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[b\](.*?)\[/b\]").unwrap());
static RE_ITALIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[i\](.*?)\[/i\]").unwrap());
static RE_REFS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(method|member|signal|param|constant|enum)\s+([^\]]+)\]").unwrap()
});
static RE_CLASSNAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[([A-Z][a-zA-Z0-9_]+)\]").unwrap());
static RE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[url[^\]]*\].*?\[/url\]").unwrap());
static RE_CODEBLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\[codeblock\].*?\[/codeblock\]").unwrap());
static RE_CODEBLOCKS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\[codeblocks\].*?\[/codeblocks\]").unwrap());
static RE_WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static RE_FIRST_SENTENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[^.!?]*[.!?]").unwrap());

/// Convert Godot BBCode markup to minimal markdown.
pub fn convert_bbcode(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let text = RE_CODE.replace_all(text, "`$1`");
    let text = RE_BOLD.replace_all(&text, "**$1**");
    let text = RE_ITALIC.replace_all(&text, "*$1*");
    let text = RE_REFS.replace_all(&text, "`$2`");
    let text = RE_CLASSNAME.replace_all(&text, "$1");
    let text = RE_URL.replace_all(&text, "");
    let text = RE_CODEBLOCK.replace_all(&text, "");
    let text = RE_CODEBLOCKS.replace_all(&text, "");
    let text = RE_WHITESPACE.replace_all(&text, " ");

    text.trim().to_string()
}

/// Extract the first sentence from BBCode text.
pub fn first_sentence(text: &str) -> String {
    let converted = convert_bbcode(text);
    if converted.is_empty() {
        return String::new();
    }

    if let Some(m) = RE_FIRST_SENTENCE.find(&converted) {
        m.as_str().trim().to_string()
    } else {
        let truncated: String = converted.chars().take(100).collect();
        truncated.trim().to_string()
    }
}
