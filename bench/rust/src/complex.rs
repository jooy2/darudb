//! What every store runs in the complex suite: three collections that link
//! to each other, the objects in them, the parameters of each row's runs,
//! and the digests a row's results are checked with. `bench/README.md`
//! describes the suite; the other languages' harnesses draw the same objects
//! and parameters, from the same hash.

/// Objects of each collection.
pub const ORGANIZATIONS: i64 = 1_000;
pub const PEOPLE: i64 = 100_000;
pub const POSTS: i64 = 300_000;

/// Objects a write transaction of the load inserts.
pub const BATCH: usize = 10_000;

pub const KINDS: [&str; 5] = ["company", "school", "hospital", "agency", "club"];

pub const COUNTRIES: [&str; 20] = [
    "kr", "us", "jp", "de", "fr", "gb", "cn", "in", "br", "ca", "au", "es", "it", "nl", "se", "mx",
    "id", "tr", "pl", "vn",
];

pub const LANGUAGES: [&str; 6] = ["en", "ko", "ja", "de", "fr", "es"];

pub const TAGS: [&str; 20] = [
    "travel", "food", "music", "sport", "news", "tech", "art", "books", "games", "film", "health",
    "science", "nature", "design", "fashion", "history", "photo", "pets", "cars", "money",
];

pub const WORDS: [&str; 64] = [
    "able", "about", "above", "across", "after", "again", "air", "almost", "along", "always",
    "animal", "answer", "around", "away", "back", "before", "began", "below", "between", "black",
    "body", "book", "both", "bring", "build", "call", "came", "carry", "change", "child", "city",
    "close", "color", "come", "country", "cover", "cross", "dark", "day", "deep", "door", "draw",
    "early", "earth", "east", "end", "even", "every", "face", "fall", "family", "far", "farm",
    "fast", "field", "fire", "first", "food", "foot", "form", "found", "free", "friend", "full",
];

/// A 32-bit integer hash, the same in every language's harness, so that
/// each field of each object is drawn from a value of its own.
pub fn hash(x: u32) -> u32 {
    let x = ((x >> 16) ^ x).wrapping_mul(0x045D_9F3B);
    let x = ((x >> 16) ^ x).wrapping_mul(0x045D_9F3B);

    (x >> 16) ^ x
}

/// The value `salt` draws for the `n`th object or run.
pub fn mix(n: i64, salt: u32) -> u32 {
    hash(hash(n as u32).wrapping_add(salt))
}

/// The `n`th of `values`, counting from `mix(n, salt)`.
fn pick<'a>(values: &[&'a str], n: i64, salt: u32) -> &'a str {
    values[mix(n, salt) as usize % values.len()]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Organization {
    pub code: String,
    pub name: String,
    pub kind: String,
    pub country: String,
    pub founded: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub age: i64,
    pub country: String,
    pub city: String,
    pub zip: String,
    pub score: f64,
    pub active: bool,
    pub joined_at: i64,
    pub organization: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Post {
    pub id: i64,
    pub author: i64,
    pub title: String,
    pub body: String,
    pub likes: i64,
    pub language: String,
    pub created_at: i64,
    pub pinned: bool,
    pub tags: Vec<String>,
}

/// The code of the `n`th organization.
pub fn code(n: i64) -> String {
    format!("org-{n:04}")
}

pub fn organization(n: i64) -> Organization {
    Organization {
        code: code(n),
        name: format!("organization {n}"),
        kind: pick(&KINDS, n, 1).to_owned(),
        country: pick(&COUNTRIES, n, 2).to_owned(),
        founded: 1900 + i64::from(mix(n, 3) % 120),
    }
}

/// The `n`th person, counting from 0. They get the key `n + 1`.
pub fn person(n: i64) -> Person {
    let tags = 1 + mix(n, 8) % 3;
    let first = mix(n, 9);

    Person {
        id: 0,
        name: format!("person {n}"),
        email: format!("{n}@example.com"),
        age: i64::from(mix(n, 1) % 80),
        country: pick(&COUNTRIES, n, 2).to_owned(),
        city: format!("city {}", mix(n, 3) % 500),
        zip: format!("{:05}", mix(n, 4) % 100_000),
        score: f64::from(mix(n, 5) % 1_000_000) / 1_000_000.0,
        active: mix(n, 6) % 7 != 0,
        // A permutation of the people, so that no two join at once.
        joined_at: 1_600_000_000 + (n * 7_907 % PEOPLE) * 37,
        organization: code(i64::from(mix(n, 7)) % ORGANIZATIONS),
        tags: (0..tags)
            .map(|at| TAGS[((first % 20 + at * 7) % 20) as usize].to_owned())
            .collect(),
    }
}

/// The `m`th post, counting from 0. It gets the key `m + 1`.
pub fn post(m: i64) -> Post {
    let tags = mix(m, 5) % 3;
    let first = mix(m, 6);

    Post {
        id: 0,
        author: 1 + i64::from(mix(m, 1)) % PEOPLE,
        title: format!("post {m}"),
        body: (0..12)
            .map(|at| WORDS[(mix(m, 100 + at) % 64) as usize])
            .collect::<Vec<_>>()
            .join(" "),
        likes: i64::from(mix(m, 3) % 1_000),
        language: pick(&LANGUAGES, m, 4).to_owned(),
        // A permutation of the posts, so that no two are made at once.
        created_at: 1_650_000_000 + (m * 104_729 % POSTS) * 60,
        pinned: mix(m, 7) % 50 == 0,
        tags: (0..tags)
            .map(|at| TAGS[((first % 20 + at * 7) % 20) as usize].to_owned())
            .collect(),
    }
}

/// The parameters of each row's runs, counting runs from 0.
pub mod runs {
    use super::{COUNTRIES, KINDS, LANGUAGES, PEOPLE, TAGS, WORDS, mix};

    /// `person-detail`: whose screen.
    pub fn detail(round: u64) -> i64 {
        1 + i64::from(mix(round as i64, 11)) % PEOPLE
    }

    /// `people-filter`: the youngest age of the ten, and the country.
    pub fn filter(round: u64) -> (i64, &'static str) {
        (
            i64::from(mix(round as i64, 12) % 70),
            COUNTRIES[(mix(round as i64, 13) % 20) as usize],
        )
    }

    /// `people-or`: the two countries.
    pub fn either(round: u64) -> (&'static str, &'static str) {
        (
            COUNTRIES[(round % 20) as usize],
            COUNTRIES[((round + 7) % 20) as usize],
        )
    }

    /// `people-prefix`: the start of the names.
    pub fn prefix(round: u64) -> String {
        format!("person {}", mix(round as i64, 14) % 1_000)
    }

    /// `people-page`: how many to skip.
    pub fn offset(round: u64) -> u64 {
        round * 997
    }

    /// `posts-author-country`: the authors' country.
    pub fn author_country(round: u64) -> &'static str {
        COUNTRIES[(round % 20) as usize]
    }

    /// `posts-two-links`: the kind of the authors' organizations, and the
    /// posts' language.
    pub fn two_links(round: u64) -> (&'static str, &'static str) {
        (KINDS[(round % 5) as usize], LANGUAGES[(round % 6) as usize])
    }

    /// `posts-tag`: the tag.
    pub fn tag(round: u64) -> &'static str {
        TAGS[(round % 20) as usize]
    }

    /// `posts-text`: two words in a row.
    pub fn phrase(round: u64) -> String {
        format!(
            "{} {}",
            WORDS[(mix(round as i64, 15) % 64) as usize],
            WORDS[(mix(round as i64, 16) % 64) as usize]
        )
    }

    /// `posts-count`: the language.
    pub fn language(round: u64) -> &'static str {
        LANGUAGES[(round % 6) as usize]
    }

    /// `update-query`: the countries whose people turn a year older.
    pub const UPDATED: [&str; 4] = ["kr", "us", "jp", "de"];

    /// `update-query`: how many people that is, which the row's time is
    /// divided by.
    pub fn updated() -> u64 {
        (0..PEOPLE)
            .filter(|&n| UPDATED.contains(&super::person(n).country.as_str()))
            .count() as u64
    }

    /// `delete-cascade`: the people deleted with their posts.
    pub fn deleted(round: u64) -> i64 {
        1 + round as i64 * 97
    }

    /// `delete-cascade`: how many.
    pub const DELETED: u64 = 1_000;
}

/// The rows of the suite, with how many runs or objects each times.
pub mod rows {
    pub const LOAD: (&str, u64) = (
        "load",
        (super::ORGANIZATIONS + super::PEOPLE + super::POSTS) as u64,
    );
    pub const DETAIL: (&str, u64) = ("person-detail", 10_000);
    pub const FILTER: (&str, u64) = ("people-filter", 500);
    pub const EITHER: (&str, u64) = ("people-or", 100);
    pub const PREFIX: (&str, u64) = ("people-prefix", 100);
    pub const PAGE: (&str, u64) = ("people-page", 100);
    pub const AUTHOR_COUNTRY: (&str, u64) = ("posts-author-country", 100);
    pub const TWO_LINKS: (&str, u64) = ("posts-two-links", 30);
    pub const TAG: (&str, u64) = ("posts-tag", 200);
    pub const TEXT: (&str, u64) = ("posts-text", 10);
    pub const COUNT: (&str, u64) = ("posts-count", 200);
    pub const UPDATE: &str = "update-query";
    pub const DELETE: &str = "delete-cascade";
}
