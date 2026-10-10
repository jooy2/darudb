//! What every store runs: the objects, the order random reads go in, the
//! rows of the table and how each is timed, and the digest a row's results
//! are checked with. `bench/README.md` describes the workloads; the other
//! languages' harnesses draw the same objects in the same order.

use std::time::Instant;

/// Objects in the read and query workloads.
pub const OBJECTS: i64 = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub age: i64,
    pub city: String,
    pub score: f64,
}

/// The `n`th object, counting from 0. It gets the key `n + 1`.
pub fn person(n: i64) -> Person {
    Person {
        id: 0,
        name: format!("person {n}"),
        email: format!("{n}@example.com"),
        age: n * 7919 % 80,
        city: format!("city {}", n % 100),
        score: (n as f64 * 0.618).fract(),
    }
}

/// Spreads consecutive numbers over 64 bits, so that reads go in an order
/// that has nothing to do with the keys'.
pub fn scatter(round: u64) -> u64 {
    round.wrapping_mul(0x9E37_79B9_7F4A_7C15).rotate_left(17)
}

/// The key of the object the `round`th random read asks for.
pub fn random_id(round: u64) -> i64 {
    1 + (scatter(round) % OBJECTS as u64) as i64
}

/// The number of the object the `round`th random email lookup asks for.
pub fn random_number(round: u64) -> i64 {
    (scatter(round) % OBJECTS as u64) as i64
}

/// How many results a row saw, and a 32-bit hash of their keys and ages in
/// the order they came, which every store has to agree on.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Digest {
    pub count: u64,
    pub hash: u32,
}

impl Digest {
    #[inline]
    pub fn add(&mut self, id: i64, age: i64) {
        self.count += 1;
        self.hash = (self.hash ^ (id * 131 + age) as u32).wrapping_mul(0x0100_0193);
    }

    #[inline]
    pub fn person(&mut self, person: &Person) {
        self.add(person.id, person.age);
    }

    pub fn number(&mut self, value: u64) {
        self.add(value as i64, 0);
    }
}

/// Probe only: a path a syscall trace shows when each row starts.
fn mark(id: &str) {
    let _ = std::fs::metadata(format!("/bench-marker/{id}"));
}

/// Probe only: `BENCH_FOCUS=<row>` runs that row `BENCH_REPEAT` times over,
/// then ends the pass, so that a profile is mostly that row.
fn focus(id: &str) -> Option<u64> {
    match std::env::var("BENCH_FOCUS") {
        Ok(focus) if focus == id => Some(
            std::env::var("BENCH_REPEAT")
                .ok()
                .and_then(|repeat| repeat.parse().ok())
                .unwrap_or(1),
        ),
        _ => None,
    }
}

pub struct Row {
    pub id: &'static str,
    pub nanos_each: f64,
    pub digest: Digest,
}

#[derive(Default)]
pub struct Rows(pub Vec<Row>);

impl Rows {
    /// Runs `step` `count` times and records the time each took on average.
    pub fn each<E: std::fmt::Debug>(
        &mut self,
        id: &'static str,
        count: u64,
        mut step: impl FnMut(u64, &mut Digest) -> Result<(), E>,
    ) {
        let mut digest = Digest::default();
        let repeat = focus(id);

        mark(id);

        let started = Instant::now();

        for _ in 0..repeat.unwrap_or(1) {
            for round in 0..count {
                step(round, &mut digest).unwrap_or_else(|error| panic!("{id}: {error:?}"));
            }
        }

        let nanos = started.elapsed().as_nanos() as f64;

        self.0.push(Row {
            id,
            nanos_each: nanos / count as f64,
            digest,
        });

        if repeat.is_some() {
            self.print();
            std::process::exit(0);
        }
    }

    /// Runs `work` once, which does `count` operations and commits them, and
    /// records the time per operation, the commit included.
    pub fn all<E: std::fmt::Debug>(
        &mut self,
        id: &'static str,
        count: u64,
        work: impl FnOnce(&mut Digest) -> Result<(), E>,
    ) {
        let mut digest = Digest::default();

        mark(id);

        let started = Instant::now();

        work(&mut digest).unwrap_or_else(|error| panic!("{id}: {error:?}"));

        let nanos = started.elapsed().as_nanos() as f64;

        self.0.push(Row {
            id,
            nanos_each: nanos / count as f64,
            digest,
        });

        if focus(id).is_some() {
            self.print();
            std::process::exit(0);
        }
    }

    /// A row that is checked and not timed.
    pub fn check(&mut self, id: &'static str, digest: Digest) {
        self.0.push(Row {
            id,
            nanos_each: 0.0,
            digest,
        });
    }

    /// One line of JSON per row, for the parent process.
    pub fn print(&self) {
        for row in &self.0 {
            println!(
                "{{\"row\":\"{}\",\"ns\":{},\"count\":{},\"hash\":{}}}",
                row.id, row.nanos_each, row.digest.count, row.digest.hash
            );
        }
    }
}

/// The record a key-value store keeps for a person, written by hand as an
/// application would: the fixed-width fields first, so that a scan reads
/// `age` and `score` without decoding the rest.
pub mod record {
    use super::Person;

    pub fn encode(person: &Person, into: &mut Vec<u8>) {
        into.clear();
        into.extend_from_slice(&person.age.to_le_bytes());
        into.extend_from_slice(&person.score.to_le_bytes());

        for text in [&person.name, &person.email, &person.city] {
            into.extend_from_slice(&(text.len() as u16).to_le_bytes());
            into.extend_from_slice(text.as_bytes());
        }
    }

    pub fn decode(id: i64, bytes: &[u8]) -> Person {
        let mut at = 16;
        let mut text = || {
            let length = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
            let value = std::str::from_utf8(&bytes[at + 2..at + 2 + length])
                .expect("utf-8")
                .to_owned();

            at += 2 + length;
            value
        };
        let name = text();
        let email = text();
        let city = text();

        Person {
            id,
            name,
            email,
            age: age(bytes),
            city,
            score: score(bytes),
        }
    }

    #[inline]
    pub fn age(bytes: &[u8]) -> i64 {
        i64::from_le_bytes(bytes[0..8].try_into().expect("8 bytes"))
    }

    #[inline]
    pub fn score(bytes: &[u8]) -> f64 {
        f64::from_le_bytes(bytes[8..16].try_into().expect("8 bytes"))
    }

    /// The city, read without decoding the strings before it.
    #[inline]
    pub fn city(bytes: &[u8]) -> &[u8] {
        let mut at = 16;

        for _ in 0..2 {
            at += 2 + u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
        }

        let length = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;

        &bytes[at + 2..at + 2 + length]
    }

    /// The age index's key: the age, then the key, both big-endian, so that
    /// the bytes sort as the numbers do.
    pub fn age_key(age: i64, id: i64) -> [u8; 16] {
        let mut key = [0; 16];

        key[..8].copy_from_slice(&(age as u64).to_be_bytes());
        key[8..].copy_from_slice(&(id as u64).to_be_bytes());
        key
    }

    pub fn id_key(id: i64) -> [u8; 8] {
        (id as u64).to_be_bytes()
    }

    pub fn id_of(key: &[u8]) -> i64 {
        u64::from_be_bytes(key[key.len() - 8..].try_into().expect("8 bytes")) as i64
    }
}

/// The ten highest scores seen, ties going to the lower key, as a query
/// sorted by score in descending order gives them.
pub struct Top {
    pub entries: Vec<(f64, i64)>,
}

impl Top {
    pub fn new() -> Self {
        Self {
            entries: Vec::with_capacity(11),
        }
    }

    #[inline]
    pub fn offer(&mut self, score: f64, id: i64) {
        let better = |a: &(f64, i64), b: &(f64, i64)| a.0 > b.0 || (a.0 == b.0 && a.1 < b.1);

        if self.entries.len() == 10 && !better(&(score, id), &self.entries[9]) {
            return;
        }

        let at = self
            .entries
            .iter()
            .position(|entry| better(&(score, id), entry))
            .unwrap_or(self.entries.len());

        self.entries.insert(at, (score, id));
        self.entries.truncate(10);
    }
}
