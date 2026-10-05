//! Every way an operation can fail, and the stable code each failure carries.
//!
//! The message of an error is for a person and may be reworded in any release.
//! The code returned by [`Error::code`] is for a program: it is the same in
//! every language binding, and once released it is never renamed. That is why
//! a binding passes the code through untouched rather than inventing its own.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// A [`Result`](std::result::Result) whose error is this crate's [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A failed database operation.
///
/// New variants may be added in any release, so a `match` on this type needs a
/// wildcard arm. Match on [`Error::code`] where a string is more convenient.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The operating system failed an operation on the file or its directory.
    Io {
        /// The file or directory the operation was on.
        path: PathBuf,
        /// What the operating system reported.
        source: io::Error,
    },
    /// No database exists at the path, and creating one was not allowed.
    NotFound {
        /// The path that was opened.
        path: PathBuf,
    },
    /// The file exists but is not a DaruDB database: it is empty, too short to
    /// hold a header, or starts with bytes that are not DaruDB's.
    NotADatabase {
        /// The file that was opened.
        path: PathBuf,
    },
    /// The file is a DaruDB database in a format this build of the library
    /// cannot read: a file format version, or an object format version of
    /// the schema stored in it, other than the one this build reads and
    /// writes. That is a file a newer build wrote, or one a development build
    /// wrote before the first release.
    UnsupportedFormatVersion {
        /// The file that was opened.
        path: PathBuf,
        /// The version recorded in the file.
        found: u32,
        /// The version this build reads and writes.
        supported: u32,
    },
    /// The file is a DaruDB database, but what it records is impossible, so
    /// part of it has been damaged.
    Corrupted {
        /// The file that was opened.
        path: PathBuf,
        /// What was found to be impossible.
        reason: String,
    },
    /// The caller asked for something that cannot be done, such as a page size
    /// that is not a power of two.
    InvalidArgument {
        /// What was wrong with the argument.
        message: String,
    },
    /// The database was used after it was closed.
    ///
    /// The Rust API cannot produce this, because [`Database::close`] consumes
    /// the handle. A language binding's handle outlives its close, and the
    /// binding reports a use after that with this error, so that its code
    /// comes from this list like every other.
    ///
    /// [`Database::close`]: crate::Database::close
    Closed,
    /// The database stayed busy for longer than the busy timeout allows:
    /// another write transaction held it, in this process or another, or
    /// another process was recovering it. Salvage needs the file alone, so
    /// it fails with this when the file is open, and opening a file that
    /// salvage is reading fails with it too.
    Busy {
        /// The database's path.
        path: PathBuf,
    },
    /// A barrier failed, so a commit's outcome is unknown. The database has to
    /// be closed and opened again before it can be used.
    SyncFailed {
        /// The database's path.
        path: PathBuf,
        /// What the operating system reported, if this is the failure itself
        /// rather than a later use of the database.
        source: Option<io::Error>,
    },
    /// The database is encrypted, and it was opened without a key or a
    /// password.
    KeyRequired {
        /// The database's path.
        path: PathBuf,
    },
    /// The key or password does not open the database.
    WrongKey {
        /// The database's path.
        path: PathBuf,
    },
    /// The database is on a network file system, or on one whose file locks
    /// do not work. Neither keeps the promises about locks and syncs that
    /// DaruDB relies on, so the database has to be on a local disk.
    UnsupportedFileSystem {
        /// The database's path.
        path: PathBuf,
    },
    /// The schema an application declared differs from the one the file holds
    /// at the same version, or another process migrated the file to a new
    /// schema since this handle opened it.
    SchemaMismatch {
        /// What differs.
        message: String,
    },
    /// The file holds a newer version of the schema than the application
    /// declared: a newer application wrote it.
    SchemaTooNew {
        /// The version the file holds.
        stored: u64,
        /// The version the application declared.
        declared: u64,
    },
    /// An insert found its primary key taken, or a unique index found its
    /// value taken.
    DuplicateKey {
        /// Which key or value, in which collection.
        message: String,
    },
    /// A query does not parse, or does not fit the schema.
    InvalidQuery {
        /// What is wrong, and where.
        message: String,
    },
    /// An application's migration function reported an error. It constructs
    /// this one to say why; an error of the engine's that it returns keeps its
    /// own code.
    MigrationFailed {
        /// Why the migration failed.
        message: String,
    },
    /// An invariant of the engine does not hold, which only a bug in DaruDB can
    /// cause. Please report it.
    Internal {
        /// What went wrong.
        message: String,
    },
}

impl Error {
    /// The stable, machine-readable name of this failure.
    ///
    /// It is the same string in every language binding, where it is exposed as
    /// the error's `code`.
    pub fn code(&self) -> &'static str {
        match self {
            Error::Io { .. } => "IO",
            Error::NotFound { .. } => "NOT_FOUND",
            Error::NotADatabase { .. } => "NOT_A_DATABASE",
            Error::UnsupportedFormatVersion { .. } => "UNSUPPORTED_FORMAT_VERSION",
            Error::Corrupted { .. } => "CORRUPTED",
            Error::InvalidArgument { .. } => "INVALID_ARGUMENT",
            Error::Closed => "CLOSED",
            Error::Busy { .. } => "BUSY",
            Error::SyncFailed { .. } => "SYNC_FAILED",
            Error::KeyRequired { .. } => "KEY_REQUIRED",
            Error::WrongKey { .. } => "WRONG_KEY",
            Error::UnsupportedFileSystem { .. } => "UNSUPPORTED_FILE_SYSTEM",
            Error::SchemaMismatch { .. } => "SCHEMA_MISMATCH",
            Error::SchemaTooNew { .. } => "SCHEMA_TOO_NEW",
            Error::DuplicateKey { .. } => "DUPLICATE_KEY",
            Error::InvalidQuery { .. } => "INVALID_QUERY",
            Error::MigrationFailed { .. } => "MIGRATION_FAILED",
            Error::Internal { .. } => "INTERNAL",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => {
                write!(f, "I/O error on `{}`: {source}", path.display())
            }
            Error::NotFound { path } => {
                write!(f, "no database exists at `{}`", path.display())
            }
            Error::NotADatabase { path } => {
                write!(f, "`{}` is not a DaruDB database", path.display())
            }
            Error::UnsupportedFormatVersion {
                path,
                found,
                supported,
            } => write!(
                f,
                "`{}` uses format version {found}, and this build reads version {supported}",
                path.display()
            ),
            Error::Corrupted { path, reason } => {
                write!(f, "`{}` is damaged: {reason}", path.display())
            }
            Error::InvalidArgument { message } => f.write_str(message),
            Error::Closed => f.write_str("the database has been closed"),
            Error::Busy { path } => write!(
                f,
                "`{}` stayed busy for longer than the busy timeout: another write transaction held it, another process was recovering it, or salvage needed it alone",
                path.display()
            ),
            Error::SyncFailed { path, source } => {
                write!(
                    f,
                    "a sync of `{}` failed, so the last commit's outcome is unknown; open the database again",
                    path.display()
                )?;

                match source {
                    Some(source) => write!(f, ": {source}"),
                    None => Ok(()),
                }
            }
            Error::KeyRequired { path } => write!(
                f,
                "`{}` is encrypted; open it with its key or password",
                path.display()
            ),
            Error::WrongKey { path } => {
                write!(f, "the key or password does not open `{}`", path.display())
            }
            Error::UnsupportedFileSystem { path } => write!(
                f,
                "`{}` is on a network file system, or on one whose file locks do not work; a database has to be on a local disk",
                path.display()
            ),
            Error::SchemaMismatch { message } => f.write_str(message),
            Error::SchemaTooNew { stored, declared } => write!(
                f,
                "the file holds schema version {stored}, newer than the version {declared} declared"
            ),
            Error::DuplicateKey { message } => f.write_str(message),
            Error::InvalidQuery { message } => write!(f, "invalid query: {message}"),
            Error::MigrationFailed { message } => write!(f, "migration failed: {message}"),
            Error::Internal { message } => {
                write!(f, "internal error, which is a bug in DaruDB: {message}")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            Error::SyncFailed {
                source: Some(source),
                ..
            } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant once, with the code it has to keep.
    ///
    /// The codes are a contract with every language binding and every program
    /// that matches on them, so this list changes only by adding to it. A new
    /// variant belongs here too.
    fn every_error() -> Vec<(Error, &'static str)> {
        let path = PathBuf::from("app.darudb");

        vec![
            (
                Error::Io {
                    path: path.clone(),
                    source: io::Error::other("disk on fire"),
                },
                "IO",
            ),
            (Error::NotFound { path: path.clone() }, "NOT_FOUND"),
            (Error::NotADatabase { path: path.clone() }, "NOT_A_DATABASE"),
            (
                Error::UnsupportedFormatVersion {
                    path: path.clone(),
                    found: 2,
                    supported: 1,
                },
                "UNSUPPORTED_FORMAT_VERSION",
            ),
            (
                Error::Corrupted {
                    path: path.clone(),
                    reason: "a reason".to_owned(),
                },
                "CORRUPTED",
            ),
            (
                Error::InvalidArgument {
                    message: "a message".to_owned(),
                },
                "INVALID_ARGUMENT",
            ),
            (Error::Closed, "CLOSED"),
            (Error::Busy { path: path.clone() }, "BUSY"),
            (
                Error::SyncFailed {
                    path: path.clone(),
                    source: None,
                },
                "SYNC_FAILED",
            ),
            (Error::KeyRequired { path: path.clone() }, "KEY_REQUIRED"),
            (Error::WrongKey { path: path.clone() }, "WRONG_KEY"),
            (
                Error::UnsupportedFileSystem { path: path.clone() },
                "UNSUPPORTED_FILE_SYSTEM",
            ),
            (
                Error::SchemaMismatch {
                    message: "a message".to_owned(),
                },
                "SCHEMA_MISMATCH",
            ),
            (
                Error::SchemaTooNew {
                    stored: 2,
                    declared: 1,
                },
                "SCHEMA_TOO_NEW",
            ),
            (
                Error::DuplicateKey {
                    message: "a message".to_owned(),
                },
                "DUPLICATE_KEY",
            ),
            (
                Error::InvalidQuery {
                    message: "a message".to_owned(),
                },
                "INVALID_QUERY",
            ),
            (
                Error::MigrationFailed {
                    message: "a message".to_owned(),
                },
                "MIGRATION_FAILED",
            ),
            (
                Error::Internal {
                    message: "a message".to_owned(),
                },
                "INTERNAL",
            ),
        ]
    }

    #[test]
    fn every_error_keeps_its_released_code() {
        for (error, code) in every_error() {
            assert_eq!(error.code(), code, "{error:?}");
        }
    }

    #[test]
    fn codes_are_unique_and_screaming_snake_case() {
        let codes: Vec<_> = every_error()
            .iter()
            .map(|(error, _)| error.code())
            .collect();

        for (index, code) in codes.iter().enumerate() {
            assert!(!codes[..index].contains(code), "`{code}` is used twice");
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "`{code}` is not SCREAMING_SNAKE_CASE"
            );
        }
    }

    #[test]
    fn a_message_names_the_file() {
        for (error, _) in every_error() {
            let named = matches!(
                error,
                Error::Io { .. }
                    | Error::NotFound { .. }
                    | Error::NotADatabase { .. }
                    | Error::UnsupportedFormatVersion { .. }
                    | Error::Corrupted { .. }
                    | Error::Busy { .. }
                    | Error::SyncFailed { .. }
                    | Error::UnsupportedFileSystem { .. }
            );

            if named {
                assert!(error.to_string().contains("app.darudb"), "{error}");
            }
        }
    }

    #[test]
    fn an_io_error_keeps_what_the_system_reported_as_its_source() {
        use std::error::Error as _;

        let error = Error::Io {
            path: PathBuf::from("app.darudb"),
            source: io::Error::other("disk on fire"),
        };

        assert_eq!(
            error.source().map(ToString::to_string).as_deref(),
            Some("disk on fire")
        );
    }
}
