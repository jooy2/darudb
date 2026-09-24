//! How a database is opened: [`OpenOptions`].

use std::path::Path;
use std::time::Duration;

use zeroize::Zeroizing;

use crate::crypto::{PasswordCost, Secret};
use crate::database::Database;
use crate::error::{Error, Result};
use crate::format::{self, Cipher, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, MIN_PAGE_SIZE};
use crate::instance::Settings;

/// Options for opening a database, in the style of [`std::fs::OpenOptions`].
///
/// ```no_run
/// use darudb::OpenOptions;
///
/// let db = OpenOptions::new().create(false).open("app.darudb")?;
/// # Ok::<(), darudb::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct OpenOptions {
    create: bool,
    page_size: u32,
    busy_timeout: Duration,
    max_unsynced_pages: u64,
    max_unsynced_time: Duration,
    secret: Option<Secret>,
    password_cost: PasswordCost,
    /// The page cipher of a new encrypted file, when the engine's own tests
    /// pin it; otherwise the one the machine prefers.
    page_cipher: Option<Cipher>,
}

impl OpenOptions {
    /// The defaults: create the database if it does not exist, with the
    /// default page size.
    pub fn new() -> Self {
        Self {
            create: true,
            page_size: DEFAULT_PAGE_SIZE,
            busy_timeout: Duration::from_secs(5),
            max_unsynced_pages: 16_384,
            max_unsynced_time: Duration::from_secs(1),
            secret: None,
            password_cost: PasswordCost::DEFAULT,
            page_cipher: None,
        }
    }

    /// Whether to create the database when nothing exists at the path.
    ///
    /// On by default. With it off, opening a path where nothing exists fails
    /// with [`Error::NotFound`]. An existing file is never replaced either way.
    pub fn create(&mut self, create: bool) -> &mut Self {
        self.create = create;
        self
    }

    /// The page size of a newly created database, in bytes.
    ///
    /// A power of two from 4096 to 65536. It only applies when the database is
    /// created: an existing file keeps the page size recorded in its header.
    /// The default, 4096, is provisional until the benchmarks settle it.
    pub fn page_size(&mut self, bytes: u32) -> &mut Self {
        self.page_size = bytes;
        self
    }

    /// How long [`Database::begin_write`] waits for a write transaction that
    /// is already running, in this process or another, and opening waits for
    /// another process that is recovering the file, before failing with
    /// [`Error::Busy`]. Five seconds by default.
    ///
    /// Every handle to a file in one process shares one instance, and the
    /// options of the handle that opened the file first apply to all of them.
    pub fn busy_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.busy_timeout = timeout;
        self
    }

    /// How many pages deferred commits may write before one of them is made
    /// durable anyway. 16384 by default: 64 MiB with 4096-byte pages.
    ///
    /// The limit bounds what a power cut can undo, and how much recovery has
    /// to check after one. The default is provisional until the benchmarks
    /// settle it.
    pub fn max_unsynced_pages(&mut self, pages: u64) -> &mut Self {
        self.max_unsynced_pages = pages;
        self
    }

    /// How long deferred commits may go without a barrier. One second by
    /// default.
    ///
    /// When the time is up, a thread the engine starts for the purpose makes
    /// them durable, as [`Database::sync`] would. If a write transaction holds
    /// the writer lock at that moment, the thread waits for it, and a deferred
    /// commit made after the time is up is made durable itself. The thread
    /// exists only while deferred commits are waiting.
    pub fn max_unsynced_time(&mut self, time: Duration) -> &mut Self {
        self.max_unsynced_time = time;
        self
    }

    /// Encrypts a new database with `key`, or opens an encrypted one with it.
    ///
    /// Every page of an encrypted database is encrypted and authenticated under
    /// a random data key, which `key` wraps: with XAES-256-GCM when the machine
    /// creating the database has AES instructions, and XChaCha20-Poly1305
    /// otherwise. Opening
    /// it without a key or password fails with [`Error::KeyRequired`], and with
    /// another one with [`Error::WrongKey`]. A plain database cannot be opened
    /// with a key, and does not become encrypted: that takes a new file.
    ///
    /// Keep the key somewhere safe, such as the operating system's keystore.
    /// Without it, the data cannot be recovered.
    pub fn key(&mut self, key: [u8; 32]) -> &mut Self {
        self.secret = Some(Secret::Key(Zeroizing::new(key)));
        self
    }

    /// Encrypts a new database with a key derived from `password`, or opens
    /// an encrypted one with it.
    ///
    /// The password is hashed with Argon2id, at the cost that
    /// [`password_hashing`](Self::password_hashing) sets, into the key that
    /// wraps the data key; otherwise it is the same as [`key`](Self::key).
    pub fn password(&mut self, password: impl AsRef<[u8]>) -> &mut Self {
        self.secret = Some(Secret::Password(Zeroizing::new(password.as_ref().to_vec())));
        self
    }

    /// How much work hashing a password takes, when a new database is
    /// encrypted with one or [`Database::set_password`] changes it: Argon2id
    /// memory in KiB, iterations, and parallelism.
    ///
    /// 19 MiB (19456 KiB), 2 and 1 by default, which takes tens of
    /// milliseconds on a current computer and fits the memory limits of mobile
    /// app extensions. More makes guessing the password slower for an attacker
    /// and opening the database slower for everyone. A file records the cost it
    /// was made with, so opening it takes that cost whatever these options say.
    /// Memory is limited to 1 GiB.
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

    /// Opens the database at `path` with these options.
    pub fn open(&self, path: impl AsRef<Path>) -> Result<Database> {
        self.validate()?;

        Database::open_with(path.as_ref(), self)
    }

    pub(crate) fn creates(&self) -> bool {
        self.create
    }

    pub(crate) fn new_page_size(&self) -> u32 {
        self.page_size
    }

    pub(crate) fn settings(&self) -> Settings {
        Settings {
            busy_timeout: self.busy_timeout,
            max_unsynced_pages: self.max_unsynced_pages,
            max_unsynced_time: self.max_unsynced_time,
            password_cost: self.password_cost,
        }
    }

    pub(crate) fn secret(&self) -> Option<&Secret> {
        self.secret.as_ref()
    }

    pub(crate) fn page_cipher(&self) -> Option<Cipher> {
        self.page_cipher
    }

    /// Pins the page cipher of a new encrypted file, so the tests cover both
    /// ciphers on any machine.
    #[cfg(test)]
    pub(crate) fn pin_page_cipher(&mut self, cipher: Cipher) -> &mut Self {
        self.page_cipher = Some(cipher);
        self
    }

    fn validate(&self) -> Result<()> {
        if !format::is_valid_page_size(self.page_size) {
            return Err(Error::InvalidArgument {
                message: format!(
                    "the page size must be a power of two from {MIN_PAGE_SIZE} to {MAX_PAGE_SIZE}, not {}",
                    self.page_size
                ),
            });
        }

        if let Some(Secret::Password(password)) = &self.secret {
            if password.is_empty() {
                return Err(Error::InvalidArgument {
                    message: "the password is empty".to_owned(),
                });
            }
        }

        self.password_cost
            .check()
            .map_err(|reason| Error::InvalidArgument {
                message: reason.to_owned(),
            })
    }
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self::new()
    }
}
