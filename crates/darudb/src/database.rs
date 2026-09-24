//! The public handle to one open database file: [`Database`].

use std::io;
use std::path::Path;
use std::sync::Arc;

use zeroize::Zeroizing;

use crate::crypto::{self, DataKey, PageCipher, RecordAuth, Secret, Unlocker};
use crate::error::{Error, Result};
use crate::format::{
    Cipher, CommitRecord, HEADER_LEN, HeaderError, KeyBlock, SELECTOR_OFFSET, SLOT_COUNT,
    STATIC_LEN, Selector, StaticHeader, slot_offset,
};
use crate::instance::{FileKey, Shared, find, registry};
use crate::lock::{Access, LockError, Locks, on_network_file_system};
use crate::options::OpenOptions;
use crate::storage::{self, Created, DbFile, FileIo, Pager};
use crate::txn::{ReadTransaction, WriteTransaction, recovery};

/// An open database.
///
/// Cloning a `Database`, or opening the same file again in the same process,
/// gives another handle to one shared instance: one file handle, one page
/// cache, and one writer at a time. The file is closed when the last handle
/// is dropped.
#[derive(Debug, Clone)]
pub struct Database {
    shared: Arc<Shared>,
}

impl Database {
    /// Opens the database at `path`, creating it if nothing exists there.
    ///
    /// The same as [`OpenOptions::new`] followed by [`OpenOptions::open`].
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        OpenOptions::new().open(path)
    }

    /// The path the database was opened at.
    pub fn path(&self) -> &Path {
        &self.shared.path
    }

    /// The size of every page in the file, in bytes.
    pub fn page_size(&self) -> u32 {
        self.shared.static_header.page_size
    }

    /// The file format version of the file, which is the one this build reads
    /// and writes: a file in any other version is refused when it is opened.
    pub fn format_version(&self) -> u32 {
        crate::FORMAT_VERSION
    }

    /// Starts a read transaction: a consistent view of the database as of the
    /// last commit.
    pub fn begin_read(&self) -> Result<ReadTransaction> {
        ReadTransaction::begin(&self.shared)
    }

    /// Starts the write transaction, waiting for one already running in
    /// another thread for up to the busy timeout.
    pub fn begin_write(&self) -> Result<WriteTransaction> {
        WriteTransaction::begin(&self.shared)
    }

    /// Makes every commit so far durable, deferred ones included, whichever
    /// process made them.
    ///
    /// It does nothing when there is no deferred commit to make durable.
    /// Otherwise it waits for a write transaction that is running, in this
    /// process or another, up to the busy timeout.
    pub fn sync(&self) -> Result<()> {
        self.shared.sync()
    }

    /// Whether the database is encrypted.
    pub fn is_encrypted(&self) -> bool {
        self.shared.data_key.is_some()
    }

    /// Changes the key of an encrypted database to `key`.
    ///
    /// The data key is wrapped anew and no page is encrypted again, so it
    /// takes three sync commits whatever the size of the database: one that
    /// writes the new key block, and two that overwrite the old one in the
    /// other commit slots. When it returns, the old key or password no longer
    /// opens the file. A plain database fails with [`Error::InvalidArgument`].
    pub fn set_key(&self, key: [u8; 32]) -> Result<()> {
        self.rekey(&Secret::Key(Zeroizing::new(key)))
    }

    /// Changes the key of an encrypted database to one derived from
    /// `password`, at the cost of the options the file was opened with; see
    /// [`set_key`](Self::set_key).
    pub fn set_password(&self, password: impl AsRef<[u8]>) -> Result<()> {
        if password.as_ref().is_empty() {
            return Err(Error::InvalidArgument {
                message: "the password is empty".to_owned(),
            });
        }

        self.rekey(&Secret::Password(Zeroizing::new(
            password.as_ref().to_vec(),
        )))
    }

    fn rekey(&self, secret: &Secret) -> Result<()> {
        let Some(data_key) = &self.shared.data_key else {
            return Err(Error::InvalidArgument {
                message: format!(
                    "`{}` is not encrypted; copying it into an encrypted database is the way to encrypt it",
                    self.path().display()
                ),
            });
        };
        let file_id = self.shared.static_header.file_id;
        let block = wrap_key(
            self.path(),
            secret,
            self.shared.settings.password_cost,
            data_key,
            &file_id,
        )?
        .encode();
        let mut txn = self.begin_write()?;

        txn.replace_key_block(block);
        txn.commit()?;

        // Every commit copies the key block of the one before it, so a few
        // empty commits carry the new block into the other slots.
        for _ in 0..2 * SLOT_COUNT {
            let stale = self
                .shared
                .header()
                .records
                .iter()
                .flatten()
                .any(|record| record.key_block != block);

            if !stale {
                return Ok(());
            }

            self.begin_write()?.commit()?;
        }

        Err(Error::Internal {
            message: "the old key block outlived the commits meant to replace it".to_owned(),
        })
    }

    /// Closes this handle, making deferred commits durable first.
    ///
    /// It reports `SYNC_FAILED` if a barrier failed on any handle to this file,
    /// which is the last chance to notice. Dropping the handle instead makes
    /// deferred commits durable too, when it is the last one, but cannot report
    /// a failure.
    pub fn close(self) -> Result<()> {
        self.sync()
    }

    /// Opens or creates the database, once the options are known to be valid.
    ///
    /// The registry of open files stays locked throughout, so two threads
    /// opening one file end up with one instance, and the process with one
    /// handle to the file.
    pub(crate) fn open_with(path: &Path, options: &OpenOptions) -> Result<Self> {
        let mut instances = registry();

        instances.retain(|_, instance| instance.strong_count() > 0);

        if let Some(shared) = FileKey::of(path).and_then(|key| find(&instances, &key)) {
            shared.admit(options.secret())?;

            return Ok(Self { shared });
        }

        // Checked before anything is created, so that a refused database
        // leaves no file behind, and again below for the file that opens.
        if options.creates() && FileKey::of(path).is_none() && on_network_file_system(path, None) {
            return Err(Error::UnsupportedFileSystem {
                path: path.to_path_buf(),
            });
        }

        let created = if options.creates() {
            create(path, options)?
        } else {
            None
        };
        // The first page of a file created empty in place, which is written
        // under the open lock.
        let mut unwritten = None;
        let (file, data_key) = match created {
            Some((Created::Whole(file), data_key, _)) => (file, data_key),
            Some((Created::Empty(file), data_key, page)) => {
                unwritten = Some(page);

                (file, data_key)
            }
            None => (open_file(path)?, None),
        };
        let file = Arc::new(file);

        if on_network_file_system(path, Some(&file)) {
            return Err(Error::UnsupportedFileSystem {
                path: path.to_path_buf(),
            });
        }

        let key = FileKey::of_file(&file, path).map_err(|source| io_error(path, source))?;

        if let Some(shared) = find(&instances, &key) {
            // The path led to a file this process has open after all: it was
            // moved there after the lookup above. Closing the new handle would
            // release the instance's locks, so the instance keeps it.
            shared.keep_handle(file);
            shared.admit(options.secret())?;

            return Ok(Self { shared });
        }

        let locks = Locks::on(Arc::clone(&file));
        let timeout = options.settings().busy_timeout;
        let access = match unwritten {
            None => locks
                .open(timeout)
                .map_err(|error| lock_error(path, error))?,
            Some(page) => {
                // Another process that opened the empty file first finds no
                // database there and lets go of the lock.
                locks
                    .open_alone(timeout)
                    .map_err(|error| lock_error(path, error))?;
                storage::fill(&file, path, &page).map_err(|source| io_error(path, source))?;

                Access::Alone
            }
        };
        let shared = open_io(file, locks, access, path, options, data_key)?;

        instances.insert(key, Arc::downgrade(&shared));

        Ok(Self { shared })
    }

    /// Opens a database whose file is `io`, bypassing the file system: the
    /// crash tests open their simulated disks with it.
    #[cfg(test)]
    pub(crate) fn open_io(io: Arc<dyn FileIo>, options: &OpenOptions) -> Result<Self> {
        Ok(Self {
            shared: open_io(
                io,
                Locks::none(),
                Access::Alone,
                Path::new("simulated.darudb"),
                options,
                None,
            )?,
        })
    }

    /// Writes a new database onto the empty `io` and opens it.
    #[cfg(test)]
    pub(crate) fn create_io(
        io: Arc<dyn FileIo>,
        page_size: u32,
        options: &OpenOptions,
    ) -> Result<Self> {
        let path = Path::new("simulated.darudb");
        let (page, data_key) = new_file(path, page_size, options)?;

        io.write_at(&page, 0)
            .map_err(|source| io_error(path, source))?;
        io.sync().map_err(|source| io_error(path, source))?;

        Ok(Self {
            shared: open_io(io, Locks::none(), Access::Alone, path, options, data_key)?,
        })
    }

    /// The instance behind this handle, for the engine's own tests.
    #[cfg(test)]
    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }
}

/// Reads the static fields of the file, unlocks an encrypted one, and builds
/// the shared instance. `data_key` is the key of a file this process has just
/// created, which need not be unwrapped again.
///
/// `locks` hold the open lock already, as `access` says. Alone, the process
/// runs recovery and then shares the lock with other processes. Otherwise
/// another process has the file open and recovered it, and only the published
/// record is checked.
fn open_io(
    io: Arc<dyn FileIo>,
    locks: Locks,
    access: Access,
    path: &Path,
    options: &OpenOptions,
    data_key: Option<DataKey>,
) -> Result<Arc<Shared>> {
    let len = io.len().map_err(|source| io_error(path, source))?;
    let header_len = usize::try_from(len).map_or(HEADER_LEN, |len| len.min(HEADER_LEN));
    let mut bytes = vec![0u8; header_len];

    io.read_at(&mut bytes, 0)
        .map_err(|source| io_error(path, source))?;

    let static_header = StaticHeader::decode(&bytes).map_err(|error| header_error(path, error))?;

    if len < u64::from(static_header.page_size) {
        return Err(Error::Corrupted {
            path: path.to_path_buf(),
            reason: format!(
                "the file is {len} bytes long, shorter than its first page of {} bytes",
                static_header.page_size
            ),
        });
    }

    let data_key = match (static_header.cipher, data_key) {
        (Cipher::Plain, _) if options.secret().is_some() => {
            return Err(Error::InvalidArgument {
                message: format!(
                    "`{}` is not encrypted, so it cannot be opened with a key",
                    path.display()
                ),
            });
        }
        (Cipher::Plain, _) => None,
        (_, Some(data_key)) => Some(data_key),
        (_, None) => Some(unlock(path, &static_header, &bytes, options)?),
    };
    let pager = Arc::new(Pager::new(
        io,
        static_header.page_size as usize,
        path.to_path_buf(),
        data_key
            .as_ref()
            .and_then(|key| PageCipher::new(static_header.cipher, key)),
    ));
    let shared = Shared::new(
        pager,
        locks,
        path.to_path_buf(),
        static_header,
        options.settings(),
        data_key,
    );

    match access {
        Access::Alone => {
            let (header, last_barrier) = recovery::recover(
                &shared.pager,
                &shared.loader,
                shared
                    .record_auth
                    .as_ref()
                    .map(|auth| (auth, &shared.static_header.file_id)),
            )?;

            shared.set_header(header);
            shared.set_last_barrier(last_barrier);
            shared.share_open_lock()?;
        }
        // The first write transaction reads the header under the writer
        // lock. Until then the instance knows no header, and no selector a
        // power cut would bring back.
        Access::Shared => {
            shared.read_published()?;
        }
    }

    Ok(Arc::new(shared))
}

/// The data key of an encrypted file, from the key block of the first record
/// that `options`' secret unwraps, newest first. The records of a file share
/// one data key, and an older record may still hold a key block from before
/// the key was changed.
fn unlock(
    path: &Path,
    header: &StaticHeader,
    bytes: &[u8],
    options: &OpenOptions,
) -> Result<DataKey> {
    let Some(secret) = options.secret() else {
        return Err(Error::KeyRequired {
            path: path.to_path_buf(),
        });
    };
    let mut records: Vec<CommitRecord> = (0..SLOT_COUNT)
        .filter_map(|slot| {
            CommitRecord::decode(slot, &bytes[slot_offset(slot)..])
                .ok()
                .flatten()
        })
        .collect();
    let mut unlocker = Unlocker::new(secret);
    let mut damage = None;

    records.sort_by_key(|record| std::cmp::Reverse(record.txn));

    for record in records {
        let block = match KeyBlock::decode(&record.key_block) {
            Ok(Some(block)) => block,
            Ok(None) => {
                damage = Some("an encrypted file's commit record has no key block");

                continue;
            }
            Err(reason) => {
                damage = Some(reason);

                continue;
            }
        };

        match unlocker.unlock(&block, &header.file_id) {
            Ok(Some(data_key)) => return Ok(data_key),
            Ok(None) => {}
            Err(reason) => damage = Some(reason),
        }
    }

    Err(match damage {
        Some(reason) => Error::Corrupted {
            path: path.to_path_buf(),
            reason: reason.to_owned(),
        },
        None => Error::WrongKey {
            path: path.to_path_buf(),
        },
    })
}

/// A database file this process created, the data key of an encrypted one,
/// and its first page.
type NewFile = (Created, Option<DataKey>, Vec<u8>);

/// Creates a database at `path`, or returns `None` if a file is already there.
///
/// See [`storage::create_file`] for why the path never holds half a database.
fn create(path: &Path, options: &OpenOptions) -> Result<Option<NewFile>> {
    let (page, data_key) = new_file(path, options.new_page_size(), options)?;

    Ok(storage::create_file(path, &page)
        .map_err(|source| io_error(path, source))?
        .map(|created| (created, data_key, page)))
}

/// Page 0 of a new database, and the data key if `options` encrypt it.
fn new_file(
    path: &Path,
    page_size: u32,
    options: &OpenOptions,
) -> Result<(Vec<u8>, Option<DataKey>)> {
    let random = |bytes: &mut [u8]| {
        getrandom::fill(bytes).map_err(|error| io_error(path, io::Error::other(error)))
    };
    let mut file_id = [0u8; 16];
    let mut first = CommitRecord::first();

    random(&mut file_id)?;

    let (cipher, data_key) = match options.secret() {
        None => (Cipher::Plain, None),
        Some(secret) => {
            let mut bytes = Zeroizing::new([0u8; 32]);

            random(bytes.as_mut_slice())?;

            let data_key = DataKey::from_bytes(*bytes);

            first.key_block = wrap_key(
                path,
                secret,
                options.settings().password_cost,
                &data_key,
                &file_id,
            )?
            .encode();
            first.mac = RecordAuth::new(&data_key).mac(&file_id, 0, &first.authenticated());

            (
                options
                    .page_cipher()
                    .unwrap_or_else(crypto::preferred_cipher),
                Some(data_key),
            )
        }
    };
    let header = StaticHeader {
        page_size,
        file_id,
        cipher,
    };

    Ok((first_page(&header, &first), data_key))
}

/// Wraps `data_key` under `secret`, with fresh randomness.
fn wrap_key(
    path: &Path,
    secret: &Secret,
    cost: crypto::PasswordCost,
    data_key: &DataKey,
    file_id: &[u8; 16],
) -> Result<KeyBlock> {
    let mut random = [0u8; 40];

    getrandom::fill(&mut random).map_err(|error| io_error(path, io::Error::other(error)))?;

    crypto::wrap(secret, cost, data_key, file_id, &random).map_err(|reason| Error::Internal {
        message: reason.to_owned(),
    })
}

/// Page 0 of a new database: the static fields, the selector pointing at
/// slot 0, and the first commit in slot 0.
fn first_page(header: &StaticHeader, first: &CommitRecord) -> Vec<u8> {
    let mut page = vec![0u8; header.page_size as usize];
    let selector = Selector {
        slot: 0,
        unsynced: false,
    };

    page[..STATIC_LEN].copy_from_slice(&header.encode());
    page[SELECTOR_OFFSET] = selector.encode();
    page[slot_offset(0)..slot_offset(1)].copy_from_slice(&first.encode(0));

    page
}

/// Opens the file already at `path`.
fn open_file(path: &Path) -> Result<DbFile> {
    DbFile::open(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Error::NotFound {
            path: path.to_path_buf(),
        },
        _ => io_error(path, error),
    })
}

/// A lock that was not taken, as the error the caller sees. A file system
/// that reports it has no working locks is one the engine cannot use.
fn lock_error(path: &Path, error: LockError) -> Error {
    match error {
        LockError::Busy => Error::Busy {
            path: path.to_path_buf(),
        },
        LockError::Io(source) if source.kind() == io::ErrorKind::Unsupported => {
            Error::UnsupportedFileSystem {
                path: path.to_path_buf(),
            }
        }
        LockError::Io(source) => io_error(path, source),
    }
}

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn header_error(path: &Path, error: HeaderError) -> Error {
    let path = path.to_path_buf();

    match error {
        HeaderError::NotADatabase => Error::NotADatabase { path },
        HeaderError::UnsupportedVersion(found) => Error::UnsupportedFormatVersion {
            path,
            found,
            supported: crate::FORMAT_VERSION,
        },
        HeaderError::Damaged(reason) => Error::Corrupted {
            path,
            reason: reason.to_owned(),
        },
    }
}
