//! Turn feed HTML into readable plain text.
//!
//! This is not a browser. It drops tags, keeps paragraph breaks, and decodes
//! the entities that show up in RSS descriptions.

pub fn html_to_text(input: &str) -> String {
    let without_script = strip_element(input, "script");
    let without_style = strip_element(&without_script, "style");
    let mut raw = String::new();
    let mut chars = without_style.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '<' {
            raw.push(ch);
            continue;
        }
        let mut tag = String::new();
        for next in chars.by_ref() {
            if next == '>' {
                break;
            }
            tag.push(next);
        }
        if is_block_tag(&tag) {
            raw.push('\n');
        }
    }
    normalize(&decode_entities(&raw))
}

fn strip_element(input: &str, name: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let open = format!("<{name}");
    let close = format!("</{name}");
    let mut out = String::new();
    let mut cursor = 0;
    while let Some(rel) = lower[cursor..].find(&open) {
        let start = cursor + rel;
        out.push_str(&input[cursor..start]);
        let Some(end_rel) = lower[start..].find(&close) else {
            return out;
        };
        let after_name = start + end_rel + close.len();
        let Some(gt) = input[after_name..].find('>') else {
            return out;
        };
        cursor = after_name + gt + 1;
    }
    out.push_str(&input[cursor..]);
    out
}

fn is_block_tag(tag: &str) -> bool {
    let name = tag
        .trim()
        .trim_start_matches('/')
        .split(|ch: char| ch.is_whitespace() || ch == '/')
        .next()
        .unwrap_or("");
    matches!(
        name.to_ascii_lowercase().as_str(),
        "br" | "p"
            | "div"
            | "li"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "article"
            | "section"
    )
}

fn decode_entities(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let token = &rest[1..end];
        if let Some(ch) = named_or_numeric(token) {
            out.push(ch);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}

fn named_or_numeric(token: &str) -> Option<char> {
    match token {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        _ => {
            if let Some(hex) = token
                .strip_prefix("#x")
                .or_else(|| token.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(digits) = token.strip_prefix('#') {
                digits.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

fn normalize(input: &str) -> String {
    let mut text = String::new();
    for line in input.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let collapsed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            text.push_str(&collapsed);
            continue;
        }
        text.push('\n');
        text.push_str(&collapsed);
    }
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_keeps_paragraphs_and_decodes_entities() {
        let text = html_to_text(
            "<p>Hello <b>there</b> &amp; friends</p><script>nope()</script><p>Second&nbsp;line</p>",
        );
        assert_eq!(text, "Hello there & friends\n\nSecond line");
        assert!(!text.contains("nope"));
    }
}
