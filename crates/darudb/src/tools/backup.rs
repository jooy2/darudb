//! Backup: a copy of one commit in a new file, written while other handles
//! and processes go on using the database (`design/tools.md`, "Backup").
//!
//! The copy reads the commit a read transaction sees and writes every entry
//! of every tree, the engine's own included, in key order into a new file,
//! which is therefore as small as the data allows. The new file stays under a
//! temporary name until it is whole and durable, and then takes the path
//! given, which it never takes from a file already there.
//!
//! Every page of the copy is written anew, so encrypting it under a data key
//! of its own costs no more than copying it: [`BackupOptions`] gives it one.

use std::fs;
use std::ops::Bound;
use std::path::Path;

use zeroize::Zeroizing;

use super::{COMMIT_BYTES, taken, write_new};
use crate::crypto::{PasswordCost, Secret};
use crate::database::Database;
use crate::error::{Error, Result};
use crate::txn::ReadTransaction;

/// How [`Database::backup_with`] writes its copy.
///
/// By default, the copy is what [`Database::backup`] writes: a copy of an
/// encrypted file is encrypted under the same data key, so the same key or
/// password opens it. A key or a password here gives the copy a new random
/// data key, which it wraps instead. Changing the key or the password of a
/// file only wraps its data key again, so this is the way to leave behind a
/// data key that may have been exposed: back up under a new key, then put the
/// copy in the old file's place. A copy of a plain database is encrypted the
/// same way.
///
/// ```no_run
/// use darudb::{BackupOptions, OpenOptions};
///
/// let db = OpenOptions::new().password("old password").open("app.darudb")?;
///
/// db.backup_with("new.darudb", BackupOptions::new().password("new password"))?;
/// # Ok::<(), darudb::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct BackupOptions {
    secret: Option<Secret>,
    password_cost: PasswordCost,
}

impl BackupOptions {
    /// The defaults: the copy keeps the file's data key, if it has one.
    pub fn new() -> Self {
        Self {
            secret: None,
            password_cost: PasswordCost::DEFAULT,
        }
    }

    /// Encrypts the copy under a new data key, which `key` wraps.
    pub fn key(&mut self, key: [u8; 32]) -> &mut Self {
        self.secret = Some(Secret::Key(Zeroizing::new(key)));
        self
    }

    /// Encrypts the copy under a new data key, which a key derived from
    /// `password` wraps, at the cost [`password_hashing`](Self::password_hashing)
    /// sets.
    pub fn password(&mut self, password: impl AsRef<[u8]>) -> &mut Self {
        self.secret = Some(Secret::Password(Zeroizing::new(password.as_ref().to_vec())));
        self
    }

    /// How much work hashing the copy's password takes: Argon2id memory in
    /// KiB, iterations, and parallelism, as
    /// [`OpenOptions::password_hashing`](crate::OpenOptions::password_hashing)
    /// says. 19 MiB, 2 and 1 by default.
    pub fn password_hashing(
        &mut self,
        memory_kib: u32,
        iterations: u32,
        parallelism: u32,
    ) -> &mut Self {
        self.password_cost = PasswordCost {
            memory_kib,
            iterations,
            parallelism,
        };
        self
    }
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// What a backup wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupReport {
    /// The transaction id of the commit copied, the one published when the
    /// backup began.
    pub commit_id: u64,
    /// The trees copied, the engine's own included.
    pub trees: u64,
    /// The entries copied.
    pub entries: u64,
    /// The size of the new file, in bytes.
    pub bytes: u64,
}

/// Copies the commit a new read transaction of `db` sees into a new file at
/// `path`, as `options` say.
pub(crate) fn backup(db: &Database, path: &Path, options: &BackupOptions) -> Result<BackupReport> {
    if matches!(&options.secret, Some(Secret::Password(password)) if password.is_empty()) {
        return Err(Error::InvalidArgument {
            message: "the password is empty".to_owned(),
        });
    }

    let read = db.begin_read()?;
    let record = *read.record();

    if fs::symlink_metadata(path).is_ok() {
        return Err(taken(path, "a backup"));
    }

    let copy = match &options.secret {
        None => db.create_copy_beside(path, record.key_block)?,
        Some(secret) => db.create_rekeyed_beside(path, secret, options.password_cost)?,
    };
    let (mut report, bytes) = write_new(path, "a backup", copy, |copy| fill(&read, copy))?;

    report.commit_id = record.txn;
    report.bytes = bytes;

    Ok(report)
}

/// Writes every entry of every tree `read` sees into `copy`, committing
/// whenever a transaction holds [`COMMIT_BYTES`], the last time durably.
fn fill(read: &ReadTransaction, copy: &Database) -> Result<BackupReport> {
    let mut report = BackupReport::default();
    let mut txn = copy.begin_write()?;
    let mut held = 0;

    for name in read.tree_names_in()? {
        let entries = read.range_in::<&[u8]>(
            &name,
            &(Bound::<&[u8]>::Unbounded, Bound::<&[u8]>::Unbounded),
            false,
        )?;
        let mut empty = true;

        report.trees += 1;

        for entry in entries {
            let (key, value) = entry?;

            txn.insert_in(&name, &key, &value)?;
            held += key.len() + value.len();
            report.entries += 1;
            empty = false;

            if held >= COMMIT_BYTES {
                txn.commit_deferred()?;
                txn = copy.begin_write()?;
                held = 0;
            }
        }

        // A tree with no entries is copied as one.
        if empty {
            txn.insert_in(&name, b"", b"")?;
            txn.remove_in(&name, b"")?;
        }
    }

    txn.commit()?;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::{Collection, Object, OpenOptions, Schema, Type};

    fn schema() -> Schema {
        Schema::new(1).collection(
            Collection::new("people")
                .field("name", Type::String)
                .with_default("age", Type::Int, 0)
                .index("age"),
        )
    }

    /// Every tree of a database by name, each with its entries in order.
    type Trees = BTreeMap<String, Vec<(Vec<u8>, Vec<u8>)>>;

    /// Every tree the database holds, the engine's own included, entry by
    /// entry.
    fn everything(db: &Database) -> Trees {
        let read = db.begin_read().unwrap();

        read.tree_names_in()
            .unwrap()
            .into_iter()
            .map(|name| {
                let entries = read
                    .range_in::<&[u8]>(
                        &name,
                        &(Bound::<&[u8]>::Unbounded, Bound::<&[u8]>::Unbounded),
                        false,
                    )
                    .unwrap()
                    .collect::<Result<Vec<_>>>()
                    .unwrap();

                (name, entries)
            })
            .collect()
    }

    fn filled(options: &OpenOptions, path: &Path) -> Database {
        let db = options.open(path).unwrap();
        let mut txn = db.begin_write().unwrap();

        {
            let mut people = txn.collection("people").unwrap();

            for n in 0..3_000 {
                people
                    .insert(
                        Object::new()
                            .with("name", format!("person {n}"))
                            .with("age", n % 70),
                    )
                    .unwrap();
            }
        }

        txn.insert("large", b"value", &vec![7; 50_000]).unwrap();
        txn.insert("emptied", b"gone", b"").unwrap();
        txn.commit().unwrap();

        // Space for the copy to leave out: deleted objects and an emptied
        // tree, which stays a tree.
        let mut txn = db.begin_write().unwrap();

        for id in (1..3_000).step_by(3) {
            txn.collection("people").unwrap().delete(id).unwrap();
        }

        txn.remove("emptied", b"gone").unwrap();
        txn.commit().unwrap();

        db
    }

    #[test]
    fn a_backup_holds_what_the_database_holds_and_opens_with_the_same_secret() {
        let dir = tempfile::tempdir().unwrap();
        let mut keyed = OpenOptions::new();
        let mut password = OpenOptions::new();
        let mut large_pages = OpenOptions::new();

        keyed.key([4; 32]);
        password
            .password("correct horse")
            .password_hashing(8 * 1024, 1, 1);
        large_pages.page_size(16_384);

        for (at, mut options) in [OpenOptions::new(), keyed, password, large_pages]
            .into_iter()
            .enumerate()
        {
            options.schema(schema());

            let source: PathBuf = dir.path().join(format!("source-{at}.darudb"));
            let target = dir.path().join(format!("backup-{at}.darudb"));
            let db = filled(&options, &source);
            let report = db.backup(&target).unwrap();
            let copy = options.open(&target).unwrap();

            assert_eq!(everything(&copy), everything(&db), "{at}");
            assert!(
                copy.begin_read()
                    .unwrap()
                    .tree_names()
                    .unwrap()
                    .contains(&"emptied".to_owned())
            );
            assert_eq!(copy.page_size(), db.page_size());
            assert_eq!(copy.is_encrypted(), db.is_encrypted());
            assert!(copy.check().unwrap().is_ok());
            assert_eq!(report.commit_id, db.begin_read().unwrap().commit_id());
            assert_eq!(report.bytes, fs::metadata(&target).unwrap().len());
            assert!(
                report.bytes < fs::metadata(&source).unwrap().len(),
                "{report:?}"
            );
            assert_eq!(
                report.entries,
                everything(&db)
                    .values()
                    .map(|entries| entries.len() as u64)
                    .sum::<u64>()
            );

            // Without the secret, an encrypted backup does not open.
            if db.is_encrypted() {
                let mut bare = OpenOptions::new();

                bare.schema(schema());
                assert_eq!(bare.open(&target).unwrap_err().code(), "KEY_REQUIRED");
            }
        }
    }

    /// A backup under a new key or password holds the same data under a new
    /// data key, and opens with it and not with the file's own secret. A plain
    /// database's backup is encrypted the same way.
    #[test]
    fn a_backup_under_a_new_secret_has_a_data_key_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let mut keyed = OpenOptions::new();

        keyed.key([4; 32]);

        for (at, mut options) in [OpenOptions::new(), keyed].into_iter().enumerate() {
            options.schema(schema());

            let db = filled(&options, &dir.path().join(format!("source-{at}.darudb")));
            let target = dir.path().join(format!("backup-{at}.darudb"));

            db.backup_with(
                &target,
                BackupOptions::new()
                    .password("a new password")
                    .password_hashing(8 * 1024, 1, 1),
            )
            .unwrap();

            let mut new = OpenOptions::new();

            new.schema(schema()).password("a new password");

            let copy = new.open(&target).unwrap();

            assert!(copy.is_encrypted(), "{at}");
            assert_eq!(everything(&copy), everything(&db), "{at}");
            assert!(copy.check().unwrap().is_ok(), "{at}");

            if let Some(old) = &db.shared().data_key {
                let new = copy.shared().data_key.as_ref().unwrap();

                assert_ne!(old.bytes(), new.bytes(), "{at}");
            }

            drop(copy);

            let refused = if db.is_encrypted() {
                "WRONG_KEY"
            } else {
                "KEY_REQUIRED"
            };

            assert_eq!(options.open(&target).unwrap_err().code(), refused, "{at}");
        }
    }

    #[test]
    fn a_backup_under_an_empty_password_is_refused_before_it_writes() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(schema());

        let db = filled(&options, &dir.path().join("source.darudb"));
        let target = dir.path().join("backup.darudb");
        let refused = db
            .backup_with(&target, BackupOptions::new().password(""))
            .unwrap_err();

        assert_eq!(refused.code(), "INVALID_ARGUMENT");
        assert!(!target.exists());
    }

    /// A backup taken while another thread commits holds one of the commits
    /// the file went through: the objects inserted one per commit, up to
    /// some point.
    #[test]
    fn a_backup_taken_during_writes_holds_one_commit() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(schema());

        let db = filled(&options, &dir.path().join("source.darudb"));
        let writer = {
            let db = db.clone();

            std::thread::spawn(move || {
                for n in 0..400 {
                    let mut txn = db.begin_write().unwrap();

                    txn.collection("people")
                        .unwrap()
                        .insert(Object::new().with("name", format!("late {n}")))
                        .unwrap();
                    txn.commit_deferred().unwrap();
                }
            })
        };
        let target = dir.path().join("backup.darudb");

        db.backup(&target).unwrap();
        writer.join().unwrap();

        let copy = options.open(&target).unwrap();
        let read = copy.begin_read().unwrap();
        let late: Vec<i64> = read
            .collection("people")
            .unwrap()
            .iter()
            .unwrap()
            .map(|object| object.unwrap())
            .filter(|object| {
                object
                    .get("name")
                    .and_then(crate::Value::as_str)
                    .is_some_and(|name| name.starts_with("late"))
            })
            .map(|object| object.get("id").and_then(crate::Value::as_int).unwrap())
            .collect();

        // Consecutive ids from the first late one: a prefix of the inserts.
        assert!(
            late.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "{late:?}"
        );
        assert!(copy.check().unwrap().is_ok());
    }

    #[test]
    fn a_backup_never_replaces_a_file_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut options = OpenOptions::new();

        options.schema(schema());

        let db = filled(&options, &dir.path().join("source.darudb"));
        let target = dir.path().join("taken.darudb");

        fs::write(&target, b"someone else's").unwrap();

        assert_eq!(db.backup(&target).unwrap_err().code(), "INVALID_ARGUMENT");
        assert_eq!(fs::read(&target).unwrap(), b"someone else's");

        let names: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();

        assert_eq!(names.len(), 2, "{names:?}");
    }
}
