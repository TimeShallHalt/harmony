#[path = "composer_text.rs"]
mod composer_text;
use crate::{ChatEmoji, ChatInsertion, MobileWindow};
#[cfg(test)]
use slint::{ModelRc, VecModel};
#[cfg(test)]
use std::rc::Rc;
use std::sync::LazyLock;
#[path = "emoji_completion.rs"]
mod completion;
pub(crate) use completion::{Context, update};

const PAGE: usize = 48;
const POPULAR: &[&str] = &[
    "👍", "❤️", "😂", "🎉", "🔥", "👀", "✨", "🙌", "💯", "🥰", "😁", "👋", "😊", "🙏", "💜", "😅",
    "😍", "🤔", "🤣", "👏", "🤗", "😎", "💀", "💪", "🥹", "😢", "✅", "⭐",
];

struct Entry {
    symbol: &'static str,
    name: &'static str,
    search: String,
}

static CATALOG: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    POPULAR
        .iter()
        .filter_map(|symbol| emojis::get(symbol))
        .chain(emojis::iter().filter(|emoji| !POPULAR.contains(&emoji.as_str())))
        .map(|emoji| Entry {
            symbol: emoji.as_str(),
            name: emoji.name(),
            search: std::iter::once(emoji.name())
                .chain(emoji.shortcodes())
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
                .replace('_', " "),
        })
        .collect()
});

#[cfg(test)]
fn results(query: &str, offset: i32) -> ModelRc<ChatEmoji> {
    let query = query
        .trim()
        .trim_matches(':')
        .to_lowercase()
        .replace('_', " ");
    let matches: Vec<_> = CATALOG
        .iter()
        .filter(|entry| {
            entry.symbol == query
                || query
                    .split_whitespace()
                    .all(|word| entry.search.contains(word))
        })
        .skip(offset.max(0) as usize)
        .take(PAGE)
        .map(|entry| ChatEmoji {
            symbol: entry.symbol.into(),
            key: entry.symbol.into(),
            name: entry.name.into(),
            ..Default::default()
        })
        .collect();
    Rc::new(VecModel::from(matches)).into()
}

fn insert(text: &str, anchor: i32, cursor: i32, symbol: &str) -> ChatInsertion {
    let (text, caret) = composer_text::replace(text, anchor, cursor, symbol);
    ChatInsertion {
        text: text.into(),
        caret,
    }
}

pub(super) fn bind(window: &MobileWindow) {
    completion::bind(window);
    window.on_insert_emoji(|text, anchor, cursor, symbol| insert(&text, anchor, cursor, &symbol));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_supports_shortcodes_and_compound_names() {
        use slint::Model;
        assert!(
            results(":thumbsup:", 0)
                .iter()
                .any(|item| item.symbol == "👍")
        );
        assert!(
            results("red heart", 0)
                .iter()
                .any(|item| item.symbol == "❤️")
        );
        assert_ne!(
            results("", 0).row_data(0).unwrap().symbol,
            results("", PAGE as i32).row_data(0).unwrap().symbol
        );
    }
}
