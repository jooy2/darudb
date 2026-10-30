//! Comparing strings while ignoring case, by Unicode's simple case folding
//! (`design/objects.md`, "The filter").
//!
//! Each character folds to one character, so two strings are equal ignoring
//! case when they have as many characters and each pair folds alike, and a
//! prefix or a suffix is compared the same way from its end. Nothing is
//! allocated for those; a search for a substring folds the string into a
//! buffer the test keeps and searches that. A character folds to itself
//! unless the table says otherwise: `ß` stays `ß`, since its full folding,
//! `ss`, is two characters, and `İ` stays `İ`, since its folding is Turkic.

use std::cell::RefCell;

use memchr::memmem::Finder;

use super::fold_table::FOLDS;

/// The character `c` folds to.
#[inline]
pub(crate) fn fold(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }

    match FOLDS.binary_search_by_key(&c, |&(from, _)| from) {
        Ok(at) => FOLDS[at].1,
        Err(_) => c,
    }
}

/// `text`, folded.
pub(crate) fn folded(text: &str) -> String {
    text.chars().map(fold).collect()
}

/// Whether `text` folds to `folded`, a string folded already.
pub(crate) fn equals(text: &str, folded: &str) -> bool {
    let mut folded = folded.chars();

    for c in text.chars() {
        if folded.next() != Some(fold(c)) {
            return false;
        }
    }

    folded.next().is_none()
}

/// Whether `text` starts with what folds to `folded`, a string folded
/// already.
pub(crate) fn starts_with(text: &str, folded: &str) -> bool {
    let mut text = text.chars();

    folded
        .chars()
        .all(|want| text.next().map(fold) == Some(want))
}

/// Whether `text` ends with what folds to `folded`, a string folded already.
pub(crate) fn ends_with(text: &str, folded: &str) -> bool {
    let mut text = text.chars().rev();

    folded
        .chars()
        .rev()
        .all(|want| text.next().map(fold) == Some(want))
}

/// A search for a substring, ignoring case: the folded text searched for,
/// and a buffer each string searched is folded into, which one query reuses
/// for every object it tests.
#[derive(Debug)]
pub(crate) struct Search {
    finder: Finder<'static>,
    buffer: RefCell<String>,
}

impl Search {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            finder: Finder::new(&folded(text)).into_owned(),
            buffer: RefCell::new(String::new()),
        }
    }

    /// Whether `text` holds the text searched for, ignoring case.
    pub(crate) fn found_in(&self, text: &str) -> bool {
        let mut buffer = self.buffer.borrow_mut();

        buffer.clear();
        buffer.extend(text.chars().map(fold));

        self.finder.find(buffer.as_bytes()).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_folds_each_character_one_way() {
        assert!(FOLDS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert!(FOLDS.iter().all(|(from, to)| from != to));

        // ASCII folds by its own rule, which the table agrees with.
        for c in '\0'..='\u{7F}' {
            let in_table = FOLDS
                .binary_search_by_key(&c, |&(from, _)| from)
                .map_or(c, |at| FOLDS[at].1);

            assert_eq!(fold(c), in_table, "{c:?}");
        }
    }

    #[test]
    fn characters_fold_by_simple_case_folding() {
        for (c, folds_to) in [
            ('A', 'a'),
            ('z', 'z'),
            ('Σ', 'σ'),
            ('ς', 'σ'),
            ('\u{212A}', 'k'),
            ('\u{1E9E}', 'ß'),
            ('ß', 'ß'),
            ('\u{0130}', '\u{0130}'),
            ('Ǆ', 'ǆ'),
            ('ǅ', 'ǆ'),
            ('\u{AB70}', '\u{13A0}'),
            ('한', '한'),
            ('Ａ', 'ａ'),
        ] {
            assert_eq!(fold(c), folds_to, "{c:?}");
        }
    }

    #[test]
    fn strings_compare_ignoring_case() {
        assert!(equals("Kim", &folded("KIM")));
        assert!(equals("ΣΟΦΟΣ", &folded("σοφος")));
        assert!(equals("\u{212A}elvin", "kelvin"));
        assert!(!equals("Kim", "ki"));
        assert!(!equals("Ki", "kim"));
        assert!(!equals("STRASSE", &folded("straße")));
        assert!(equals("", ""));

        assert!(starts_with("Hello World", &folded("hELLO")));
        assert!(!starts_with("He", &folded("hello")));
        assert!(starts_with("anything", ""));
        assert!(ends_with("Hello World", &folded("WORLD")));
        assert!(!ends_with("ld", &folded("world")));
        assert!(ends_with("Σίσυφος", &folded("ΟΣ")));

        let search = Search::new("LO wO");

        assert!(search.found_in("Hello World"));
        assert!(!search.found_in("Hello"));
        assert!(search.found_in("HELLO WORLD"));
        assert!(Search::new("").found_in("x"));
    }
}
