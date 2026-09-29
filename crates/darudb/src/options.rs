//! How a database is opened: [`OpenOptions`].

use std::path::Path;
use std::time::Duration;

use zeroize::Zeroizing;

use crate::crypto::{PasswordCost, Secret};
use crate::database::{Database, Opening};
use crate::error::{Error, Result};
use crate::format::{self, Cipher, DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, MIN_PAGE_SIZE};
use crate::instance::Settings;
use crate::schema::{self, Migration, Schema};

/// The memory the page cache of a file may take by default, in bytes.
const DEFAULT_CACHE_SIZE: usize = 32 << 20;

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
    cache_size: usize,
    max_unsynced_pages: u64,
    max_unsynced_time: Duration,
    secret: Option<Secret>,
    password_cost: PasswordCost,
    /// The page cipher of a new encrypted file, when the engine's own tests
    /// pin it; otherwise the one the machine prefers.
    page_cipher: Option<Cipher>,
    schema: Option<Schema>,
    migrations: Vec<Migration>,
}

impl OpenOptions {
    /// The defaults: create the database if it does not exist, with the
    /// default page size.
    pub fn new() -> Self {
        Self {
            create: true,
            page_size: DEFAULT_PAGE_SIZE,
            busy_timeout: Duration::from_secs(5),
            cache_size: DEFAULT_CACHE_SIZE,
            max_unsynced_pages: 16_384,
            max_unsynced_time: Duration::from_secs(1),
            secret: None,
            password_cost: PasswordCost::DEFAULT,
            page_cipher: None,
            schema: None,
            migrations: Vec::new(),
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

    /// How much memory the file's page cache may take, in bytes. 32 MiB by
    /// default.
    ///
    /// The cache keeps pages read from the file, checked and decrypted, so
    /// that reading one again costs neither a read nor a check. It holds as
    /// many pages as fit in `bytes`, and at least 16 whatever `bytes` says, and
    /// it fills only as pages are read, so a database smaller than the cache
    /// never takes all of it. A larger cache speeds up reading a database
    /// that does not fit in it; a process with little memory, such as a
    /// mobile app extension, can give it less.
    ///
    /// Every handle to a file in one process shares one cache, and the
    /// options of the handle that opened the file first apply to all of them.
    pub fn cache_size(&mut self, bytes: usize) -> &mut Self {
        self.cache_size = bytes;
        self
    }

    /// How many pages deferred commits may write before one of them is made
    /// durable anyway, each page counted once however often they write it.
    /// 16384 by default: 64 MiB with 4096-byte pages.
    ///
    /// The limit bounds how much the barrier that makes them durable has to
    /// write, and how much recovery has to check after a power cut. Deferred
    /// commits reuse the pages they wrote before, so small ones that change
    /// the same objects write the same pages again, and count them once. The
    /// default is provisional until the benchmarks settle it.
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

    /// Declares the collections of the database, and what their objects hold,
    /// at a version.
    ///
    /// Opening a file stores the schema in it the first time. Later, a file
    /// holding the same version opens if the schema is the same, and fails
    /// with [`Error::SchemaMismatch`] if it is not: a changed schema needs a
    /// new version. A file holding an older version is migrated to this one
    /// ([`migration`](Self::migration)) before the open returns, and one
    /// holding a newer version fails with [`Error::SchemaTooNew`].
    ///
    /// Without a schema, the database has no collections, and only its trees
    /// of bytes are reachable.
    ///
    /// ```no_run
    /// use darudb::{Collection, Object, OpenOptions, Schema, Type};
    ///
    /// let schema = Schema::new(1).collection(
    ///     Collection::new("users")
    ///         .field("name", Type::String)
    ///         .optional("email", Type::String)
    ///         .unique("email"),
    /// );
    /// let db = OpenOptions::new().schema(schema).open("app.darudb")?;
    /// let mut txn = db.begin_write()?;
    /// let id = txn
    ///     .collection("users")?
    ///     .insert(Object::new().with("name", "Alice"))?;
    ///
    /// txn.commit()?;
    ///
    /// let read = db.begin_read()?;
    /// let alice = read.collection("users")?.get(id)?;
    ///
    /// assert_eq!(alice.and_then(|user| user.get("name").cloned()), Some("Alice".into()));
    /// # Ok::<(), darudb::Error>(())
    /// ```
    pub fn schema(&mut self, schema: Schema) -> &mut Self {
        self.schema = Some(schema);
        self
    }

    /// Adds a migration: what schema version `n` changes from version `n − 1`
    /// beyond what the engine works out by itself. Opening a file holding an
    /// older version runs the migrations up to the declared version, in
    /// order, in one write transaction; see [`Migration`].
    pub fn migration(&mut self, migration: Migration) -> &mut Self {
        self.migrations.push(migration);
        self
    }

    /// Opens the database at `path` with these options.
    pub fn open(&self, path: impl AsRef<Path>) -> Result<Database> {
        self.validate()?;

        Database::open_with(path.as_ref(), self)
    }

    /// Opens the database at `path` like [`open`](Self::open), except that a
    /// migration stops for the caller between its version steps: see
    /// [`PendingMigration`]. A language binding runs its migration functions
    /// this way.
    ///
    /// [`PendingMigration`]: crate::PendingMigration
    pub fn open_migrating(&self, path: impl AsRef<Path>) -> Result<Opening> {
        self.validate()?;

        Database::opening(path.as_ref(), self)
    }

    /// Rescues what it can of the damaged database at `from` into a new
    /// database at `into`, and returns what it rescued and what it could
    /// not; see [`SalvageReport`](crate::SalvageReport).
    ///
    /// It reads the file page by page rather than opening it, so it works on
    /// a file that does not open. Of these options it uses the key or
    /// password, for an encrypted file, and the busy timeout. It starts from
    /// the newest commit the file records, and takes what that commit cannot
    /// read from older versions of the same pages where the file still has
    /// them. The new file gets every index
    /// built again from its objects, so it passes the integrity check, and it
    /// has the file's page size, cipher and key.
    ///
    /// It needs the file alone: a file open in this process or another fails
    /// with [`Error::Busy`](crate::Error::Busy). Like a backup, the new file
    /// is durable when this returns, and never replaces a file already at
    /// `into`, which fails with [`Error::InvalidArgument`](crate::Error::InvalidArgument).
    pub fn salvage(
        &self,
        from: impl AsRef<Path>,
        into: impl AsRef<Path>,
    ) -> Result<crate::SalvageReport> {
        self.validate()?;

        crate::tools::salvage(from.as_ref(), into.as_ref(), self)
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
            cache_size: self.cache_size,
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

    /// The declared schema and its migrations, if there is a schema.
    pub(crate) fn declared(&self) -> Option<(&Schema, &[Migration])> {
        self.schema
            .as_ref()
            .map(|schema| (schema, self.migrations.as_slice()))
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

        if let Some((declared, migrations)) = self.declared() {
            schema::check(declared, migrations)?;
        } else if !self.migrations.is_empty() {
            return Err(Error::InvalidArgument {
                message: "migrations are declared without a schema to migrate to".to_owned(),
            });
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
