//! The terms of a text: what a text index holds an entry for, and what a
//! search for words looks for (`design/objects.md`, "Text indexes").
//!
//! A text is split into **words**, runs of letters, digits and marks, each
//! folded as a comparison that ignores case folds it and cut to its first
//! [`MAX_TERM`] characters. The scripts written without spaces between
//! words, Han, hiragana and katakana, have no runs to take: a run of their
//! characters gives each two that stand side by side, as one term, and its
//! last character alone, so that a search for one character finds it
//! anywhere in a run, as the start of a pair or as the run's end.
//!
//! Which character is a letter is read from tables made from one version of
//! Unicode's data, not from the standard library's, whose version is the
//! compiler's: the terms an index holds have to be the terms every build
//! looks for.

use std::cell::RefCell;
use std::cmp::Ordering;

use super::fold_table::FOLDS;
use super::word_table::{PAIRS, WORDS};

/// The most characters of a word that make its term. A longer word is
/// known by its first characters, in an index and in a search alike, so
/// that every term fits in a key.
pub(crate) const MAX_TERM: usize = 64;

/// The character `c` folds to, by Unicode's simple case folding.
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

/// What a character is to a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// A letter, a digit or a mark: part of a word.
    Word,
    /// A letter of a script written without spaces, taken in pairs.
    Pair,
    /// Anything else, which separates words.
    Separator,
}

fn class(c: char) -> Class {
    if c.is_ascii() {
        return if c.is_ascii_alphanumeric() {
            Class::Word
        } else {
            Class::Separator
        };
    }

    let point = u32::from(c);

    if within(PAIRS, point) {
        Class::Pair
    } else if within(WORDS, point) {
        Class::Word
    } else {
        Class::Separator
    }
}

fn within(ranges: &[(u32, u32)], point: u32) -> bool {
    ranges
        .binary_search_by(|&(first, last)| {
            if last < point {
                Ordering::Less
            } else if first > point {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// A run of a text: a word, cut and folded, or the characters of a run
/// taken in pairs, folded.
enum Run<'r> {
    Word(&'r str),
    Pairs(&'r [char]),
}

/// Gives `take` each run of `text` in order, with whether a `*` follows it
/// at once, until `take` returns false.
fn runs(text: &str, mut take: impl FnMut(Run<'_>, bool) -> bool) {
    let mut word = String::new();
    let mut length = 0;
    let mut pairs: Vec<char> = Vec::new();

    for c in text.chars() {
        let class = class(c);
        let starred = c == '*';

        if class != Class::Word && !word.is_empty() {
            if !take(Run::Word(&word), starred) {
                return;
            }

            word.clear();
            length = 0;
        }

        if class != Class::Pair && !pairs.is_empty() {
            if !take(Run::Pairs(&pairs), starred) {
                return;
            }

            pairs.clear();
        }

        match class {
            Class::Word if length < MAX_TERM => {
                word.push(fold(c));
                length += 1;
            }
            Class::Pair => pairs.push(fold(c)),
            _ => {}
        }
    }

    if !word.is_empty() {
        take(Run::Word(&word), false);
    } else if !pairs.is_empty() {
        take(Run::Pairs(&pairs), false);
    }
}

/// The two characters `a` and `b` as one term, in `buffer`.
fn pair(a: char, b: char, buffer: &mut [u8; 8]) -> &str {
    let first = a.encode_utf8(buffer).len();
    let second = b.encode_utf8(&mut buffer[first..]).len();

    // Two characters encoded one after the other are UTF-8.
    std::str::from_utf8(&buffer[..first + second]).unwrap_or_default()
}

/// Gives `take` each term of `text`, in the order the text has them and
/// with the repeats it has, until `take` returns false: every word, every
/// two characters side by side in a run taken in pairs, and the last
/// character of such a run.
pub(crate) fn terms(text: &str, mut take: impl FnMut(&str) -> bool) {
    runs(text, |run, _| match run {
        Run::Word(word) => take(word),
        Run::Pairs(characters) => {
            let mut buffer = [0; 8];

            for two in characters.windows(2) {
                if !take(pair(two[0], two[1], &mut buffer)) {
                    return false;
                }
            }

            match characters.last() {
                Some(last) => take(last.encode_utf8(&mut buffer)),
                None => true,
            }
        }
    });
}

/// The words a search looks for (`design/objects.md`, "Searching for
/// words"): a text holds them when it has every term of `exact` and, for
/// each of `prefixes`, a term that starts with it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Words {
    /// Sorted, without repeats.
    exact: Vec<String>,
    /// Sorted, without repeats.
    prefixes: Vec<String>,
}

impl Words {
    /// The words of the search `text`. A word followed by `*` is the start
    /// of a word. A run taken in pairs gives its pairs, and one of a single
    /// character the start of a term, since a text has that character in a
    /// pair or as the last of a run; a `*` after such a run changes nothing.
    pub(crate) fn of(text: &str) -> Self {
        let mut words = Self::default();

        runs(text, |run, starred| {
            match run {
                Run::Word(word) if starred => words.prefixes.push(word.to_owned()),
                Run::Word(word) => words.exact.push(word.to_owned()),
                Run::Pairs([only]) => words.prefixes.push(only.to_string()),
                Run::Pairs(characters) => {
                    let mut buffer = [0; 8];

                    for two in characters.windows(2) {
                        words
                            .exact
                            .push(pair(two[0], two[1], &mut buffer).to_owned());
                    }
                }
            }

            true
        });

        words.exact.sort_unstable();
        words.exact.dedup();
        words.prefixes.sort_unstable();
        words.prefixes.dedup();
        words
    }

    /// Whether the search has no word, and so finds nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.exact.is_empty() && self.prefixes.is_empty()
    }

    /// The terms a text has to have.
    pub(crate) fn exact(&self) -> &[String] {
        &self.exact
    }

    /// The starts a term of the text has to have, one term for each.
    pub(crate) fn prefixes(&self) -> &[String] {
        &self.prefixes
    }

    /// How many terms and starts the search has.
    pub(crate) fn len(&self) -> usize {
        self.exact.len() + self.prefixes.len()
    }
}

/// A search for words in one text after another: the words, and which of
/// them the text being searched has shown so far, kept between texts so
/// that a query allocates it once.
#[derive(Debug)]
pub(crate) struct Search {
    words: Words,
    found: RefCell<Vec<bool>>,
}

impl Search {
    pub(crate) fn new(words: Words) -> Self {
        Self {
            found: RefCell::new(vec![false; words.len()]),
            words,
        }
    }

    pub(crate) fn words(&self) -> &Words {
        &self.words
    }

    /// Whether `text` has every word of the search. A search without a word
    /// finds nothing.
    pub(crate) fn found_in(&self, text: &str) -> bool {
        if self.words.is_empty() {
            return false;
        }

        let mut found = self.found.borrow_mut();
        let mut left = found.len();

        found.fill(false);

        terms(text, |term| {
            if let Ok(at) = self
                .words
                .exact
                .binary_search_by(|word| word.as_str().cmp(term))
            {
                if !found[at] {
                    found[at] = true;
                    left -= 1;
                }
            }

            let first = self.words.exact.len();

            for (at, prefix) in self.words.prefixes.iter().enumerate() {
                if !found[first + at] && term.starts_with(prefix.as_str()) {
                    found[first + at] = true;
                    left -= 1;
                }
            }

            left != 0
        });

        left == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms_of(text: &str) -> Vec<String> {
        let mut found = Vec::new();

        terms(text, |term| {
            found.push(term.to_owned());

            true
        });

        found
    }

    fn words(text: &str) -> (Vec<String>, Vec<String>) {
        let words = Words::of(text);

        (words.exact, words.prefixes)
    }

    #[test]
    fn the_tables_are_sorted_and_apart() {
        for table in [WORDS, PAIRS] {
            assert!(table.iter().all(|(first, last)| first <= last));
            assert!(table.windows(2).all(|two| two[0].1 < two[1].0));
        }

        assert!(
            PAIRS
                .iter()
                .all(|&(first, last)| !within(WORDS, first) && !within(WORDS, last))
        );
        assert!(WORDS.iter().all(|&(first, _)| first > 0x7F));
    }

    #[test]
    fn a_text_gives_its_words_folded() {
        assert_eq!(
            terms_of("Hello, WORLD! It's 3.14"),
            ["hello", "world", "it", "s", "3", "14"]
        );
        assert_eq!(terms_of("  \t\n"), [""; 0]);
        assert_eq!(terms_of(""), [""; 0]);
        assert_eq!(
            terms_of("snake_case-and.dots"),
            ["snake", "case", "and", "dots"]
        );
        assert_eq!(terms_of("Straße ΣΟΦΟΣ"), ["straße", "σοφοσ"]);
        assert_eq!(terms_of("빠른 검색을 한다."), ["빠른", "검색을", "한다"]);
        // A combining mark stays in its word, whether or not it is a letter.
        assert_eq!(terms_of("cafe\u{301} हिन्दी"), ["cafe\u{301}", "हिन्दी"]);
        assert_eq!(terms_of("a\u{2014}b\u{3000}c"), ["a", "b", "c"]);
    }

    #[test]
    fn a_long_word_is_its_first_characters() {
        let long = "é".repeat(MAX_TERM + 9);

        assert_eq!(terms_of(&long), ["é".repeat(MAX_TERM)]);
        assert_eq!(
            terms_of(&format!("{long} x")),
            ["é".repeat(MAX_TERM), "x".to_owned()]
        );
        assert_eq!(words(&format!("{long}*")).1, ["é".repeat(MAX_TERM)]);
    }

    #[test]
    fn scripts_without_spaces_go_in_pairs() {
        assert_eq!(terms_of("東京都"), ["東京", "京都", "都"]);
        assert_eq!(terms_of("日"), ["日"]);
        assert_eq!(
            terms_of("コーヒーを飲む"),
            ["コー", "ーヒ", "ヒー", "ーを", "を飲", "飲む", "む"]
        );
        // A word and a run in pairs end each other.
        assert_eq!(
            terms_of("iPhone15を買う"),
            ["iphone15", "を買", "買う", "う"]
        );
        assert_eq!(terms_of("東京2020年"), ["東京", "京", "2020", "年"]);
        assert_eq!(terms_of("東京、大阪"), ["東京", "京", "大阪", "阪"]);
    }

    #[test]
    fn a_search_reads_its_words_and_their_stars() {
        assert_eq!(
            words("Quick brown* fox"),
            (
                vec!["fox".to_owned(), "quick".to_owned()],
                vec!["brown".to_owned()]
            )
        );
        assert_eq!(words("a a A"), (vec!["a".to_owned()], Vec::new()));
        assert_eq!(words("* ** -- *x"), (vec!["x".to_owned()], Vec::new()));
        assert_eq!(words("x *"), (vec!["x".to_owned()], Vec::new()));
        assert!(Words::of("").is_empty());
        assert!(Words::of("?! *").is_empty());

        // A run in pairs gives its pairs, and one character the start of a
        // term; a star after either changes nothing.
        assert_eq!(
            words("東京都"),
            (vec!["京都".to_owned(), "東京".to_owned()], Vec::new())
        );
        assert_eq!(words("東京都*"), words("東京都"));
        assert_eq!(words("京"), (Vec::new(), vec!["京".to_owned()]));
        assert_eq!(words("京*"), words("京"));
        assert_eq!(
            words("검색* 東"),
            (Vec::new(), vec!["東".to_owned(), "검색".to_owned()])
        );
    }

    #[test]
    fn a_search_finds_the_texts_with_every_word() {
        let found = |search: &str, text: &str| Search::new(Words::of(search)).found_in(text);

        assert!(found("quick fox", "The QUICK brown fox."));
        assert!(found("fox quick", "The QUICK brown fox."));
        assert!(!found("quick cat", "The QUICK brown fox."));
        assert!(!found("qui", "The QUICK brown fox."));
        assert!(found("qui*", "The QUICK brown fox."));
        assert!(found("qui* bro* the", "The QUICK brown fox."));
        assert!(!found("qui* bra*", "The QUICK brown fox."));
        assert!(!found("", "The QUICK brown fox."));
        assert!(!found("?", "?"));
        assert!(!found("x", ""));

        assert!(found("검색*", "빠른 검색을 한다"));
        assert!(!found("검색", "빠른 검색을 한다"));
        assert!(found("검색을", "빠른 검색을 한다"));

        assert!(found("東京", "東京都に住む"));
        assert!(found("京都", "東京都に住む"));
        assert!(found("都", "東京都"));
        assert!(found("東", "東京都"));
        assert!(found("住", "東京都に住む"));
        assert!(!found("大阪", "東京都に住む"));
        assert!(!found("阪", "東京都に住む"));
        assert!(found("東京 住む", "東京都に住む"));

        // One search tests text after text.
        let search = Search::new(Words::of("a b*"));

        assert!(search.found_in("b a"));
        assert!(!search.found_in("a"));
        assert!(search.found_in("bb a"));
        assert!(!search.found_in("b"));
    }
}
