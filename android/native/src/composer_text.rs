#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    pub(crate) prefix: char,
    pub(crate) query: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn token(text: &str, anchor: i32, caret: i32) -> Option<Token> {
    let caret = usize::try_from(caret).ok()?;
    if anchor < 0 || anchor as usize != caret || !text.is_char_boundary(caret) {
        return None;
    }
    let valid = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let (start, prefix) = text[..caret]
        .char_indices()
        .rev()
        .find(|(_, c)| !valid(*c))?;
    if !matches!(prefix, '@' | '#' | ':') {
        return None;
    }
    // Do not complete inside email addresses, URLs, existing wire mentions, or emoji markup.
    if text[..start]
        .chars()
        .next_back()
        .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | '[' | '{' | '"' | '“' | '‘'))
    {
        return None;
    }
    let query = &text[start + prefix.len_utf8()..caret];
    if query.len() > 64 || (prefix == ':' && query.is_empty()) {
        return None;
    }
    let end = text[caret..]
        .char_indices()
        .find_map(|(n, c)| (!valid(c)).then_some(caret + n))
        .unwrap_or(text.len());
    Some(Token {
        prefix,
        query: query.to_lowercase(),
        start,
        end,
    })
}

// Slint TextInput passes both endpoints as UTF-8 byte offsets.
pub(crate) fn replace(text: &str, anchor: i32, cursor: i32, value: &str) -> (String, i32) {
    let boundary = |index: i32| text.floor_char_boundary((index.max(0) as usize).min(text.len()));
    let start = boundary(anchor.min(cursor));
    let end = boundary(anchor.max(cursor));
    let mut updated = text.to_owned();
    updated.replace_range(start..end, value);
    (
        updated,
        i32::try_from(start + value.len()).unwrap_or(i32::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completes_only_the_token_at_a_valid_utf8_caret() {
        let text = "é @Mira!";
        let got = token(text, 5, 5).unwrap();
        assert_eq!(
            (&text[got.start..got.end], got.query.as_str()),
            ("@Mira", "m")
        );
        assert!(token("me@example.org", 5, 5).is_none());
        assert!(token("<@123>", 4, 4).is_none());
        assert!(token(":wave", 2, 5).is_none());
        assert!(token("🌛", 5, 5).is_none());
    }

    #[test]
    fn inserts_and_replaces_at_utf8_offsets() {
        assert_eq!(replace("é親!", 2, 2, "😘"), ("é😘親!".into(), 6));
        assert_eq!(replace("abc🤔親", 7, 2, "😴"), ("ab😴親".into(), 6));
        assert_eq!(replace("é界", -1, 999, "A"), ("A".into(), 1));
    }

    #[test]
    fn completes_in_the_middle_without_losing_punctuation() {
        let text = "Try (#genral), please";
        let caret = text.find("ral").unwrap() as i32;
        let active = token(text, caret, caret).unwrap();
        assert_eq!(active.prefix, '#');
        let (completed, _) = replace(text, active.start as i32, active.end as i32, "#general");
        assert_eq!(completed, "Try (#general), please");
        assert!(token("https://discord.com", 6, 6).is_none());
        assert!(token("🙂@name", 3, 3).is_none());
    }
}
