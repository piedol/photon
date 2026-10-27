//! Release notes from Markdown into lines an app can draw: headings, bullets
//! and paragraphs, with bold and code spans. Links keep their text; other
//! Markdown is shown as written.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    Plain,
    Bold,
    Code,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Heading(Vec<Span>),
    Bullet(Vec<Span>),
    Text(Vec<Span>),
    /// A blank line between blocks.
    Gap,
}

pub fn parse(markdown: &str) -> Vec<Line> {
    // Lines of one paragraph or bullet are joined first, then the inline markup is read.
    enum Block {
        Heading(String),
        Bullet(String),
        Text(String),
        Code(String),
        Gap,
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut in_code = false;
    for raw in markdown.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            blocks.push(Block::Code(line.to_string()));
            continue;
        }
        let t = line.trim();
        if t.is_empty() {
            if !matches!(blocks.last(), None | Some(Block::Gap)) {
                blocks.push(Block::Gap);
            }
        } else if let Some(h) = heading(t) {
            blocks.push(Block::Heading(h.to_string()));
        } else if let Some(b) = ["- ", "* ", "+ "].iter().find_map(|m| t.strip_prefix(m)) {
            blocks.push(Block::Bullet(b.trim().to_string()));
        } else {
            match blocks.last_mut() {
                // A wrapped line continues the paragraph or bullet above it.
                Some(Block::Text(p) | Block::Bullet(p)) => {
                    p.push(' ');
                    p.push_str(t);
                }
                _ => blocks.push(Block::Text(t.to_string())),
            }
        }
    }
    if matches!(blocks.last(), Some(Block::Gap)) {
        blocks.pop();
    }
    blocks
        .into_iter()
        .map(|b| match b {
            Block::Heading(s) => Line::Heading(inline(&s)),
            Block::Bullet(s) => Line::Bullet(inline(&s)),
            Block::Text(s) => Line::Text(inline(&s)),
            Block::Code(s) => Line::Text(vec![Span { text: s, style: Style::Code }]),
            Block::Gap => Line::Gap,
        })
        .collect()
}

/// "## Changes" → "Changes".
fn heading(line: &str) -> Option<&str> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    line[hashes..].strip_prefix(' ').map(|h| h.trim().trim_end_matches('#').trim())
}

/// `**bold**`, `` `code` ``, `[text](url)` and `![image](url)` (dropped).
fn inline(s: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut plain = String::new();
    let flush = |plain: &mut String, spans: &mut Vec<Span>| {
        if !plain.is_empty() {
            spans.push(Span { text: std::mem::take(plain), style: Style::Plain });
        }
    };
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '\\' && rest.len() > 1 {
            let next = rest[1..].chars().next().unwrap_or('\\');
            plain.push(next);
            rest = &rest[1 + next.len_utf8()..];
            continue;
        }
        let styled = [("**", Style::Bold), ("__", Style::Bold), ("`", Style::Code)].iter().find_map(|(mark, style)| {
            let body = rest.strip_prefix(mark)?;
            let end = body.find(mark).filter(|&e| e > 0)?;
            Some((&body[..end], *style, mark.len() * 2 + end))
        });
        if let Some((text, style, used)) = styled {
            flush(&mut plain, &mut spans);
            spans.push(Span { text: text.to_string(), style });
            rest = &rest[used..];
            continue;
        }
        if let Some((text, used, image)) = link(rest) {
            if !image {
                plain.push_str(text);
            }
            rest = &rest[used..];
            continue;
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut plain, &mut spans);
    spans
}

/// `[text](url)` or `![alt](url)` at the start of `s`: (text, bytes used, is an image).
fn link(s: &str) -> Option<(&str, usize, bool)> {
    let image = s.starts_with("![");
    let open = if image { 2 } else if s.starts_with('[') { 1 } else { return None };
    let close = s[open..].find("](")? + open;
    let end = s[close + 2..].find(')')? + close + 2;
    Some((&s[open..close], end + 1, image))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(text: &str) -> Span {
        Span { text: text.into(), style: Style::Plain }
    }

    fn bold(text: &str) -> Span {
        Span { text: text.into(), style: Style::Bold }
    }

    #[test]
    fn release_notes_become_lines() {
        let md = "Tamka 0.1.1 adds a microphone choice.\r\n\r\n## Download\n\n`Tamka-macOS-arm64.zip` is for\nApple silicon.\n\n\n## Changes\n\n- **Choose the microphone.** The menu has it.\n- Mics are [mixed](https://example.com) down.\n  Both are heard.\n";
        assert_eq!(
            parse(md),
            vec![
                Line::Text(vec![plain("Tamka 0.1.1 adds a microphone choice.")]),
                Line::Gap,
                Line::Heading(vec![plain("Download")]),
                Line::Gap,
                Line::Text(vec![Span { text: "Tamka-macOS-arm64.zip".into(), style: Style::Code }, plain(" is for Apple silicon.")]),
                Line::Gap,
                Line::Heading(vec![plain("Changes")]),
                Line::Gap,
                Line::Bullet(vec![bold("Choose the microphone."), plain(" The menu has it.")]),
                Line::Bullet(vec![plain("Mics are mixed down. Both are heard.")]),
            ]
        );
    }

    #[test]
    fn odd_markup_is_left_readable() {
        assert_eq!(inline("a ** b"), vec![plain("a ** b")]);
        assert_eq!(inline("snake_case and 2*3*4"), vec![plain("snake_case and 2*3*4")]);
        assert_eq!(inline(r"\*\*not bold\*\*"), vec![plain("**not bold**")]);
        assert_eq!(inline("![shot](a.png)Done"), vec![plain("Done")]);
        assert_eq!(inline("[unclosed](x"), vec![plain("[unclosed](x")]);
        assert_eq!(parse("#hashtag"), vec![Line::Text(vec![plain("#hashtag")])]);
        assert_eq!(parse("```\nlet x = 1;\n```"), vec![Line::Text(vec![Span { text: "let x = 1;".into(), style: Style::Code }])]);
        assert!(parse("\n\n").is_empty());
    }
}
