/// The synchronous API: opening a database, transactions scoped to a
/// function, and the collections a transaction reads and writes.
///
/// Every call reaches the engine through `dart:ffi` and holds the isolate
/// until it returns, as a call of the engine's own Rust API would. A write
/// transaction waits for another writer, up to the busy timeout, and a sync
/// commit for the disk, so a Flutter app keeps them off its UI isolate.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:ffi';
import 'dart:io' show File;
import 'dart:typed_data';

import 'package:ffi/ffi.dart';

import 'codec.dart';
import 'errors.dart';
import 'native.dart';
import 'query.dart';
import 'schema.dart';

part 'async.dart';
part 'tools.dart';

/// When a commit is durable.
enum Durability {
  /// The commit is durable when it returns: a crash or a power cut keeps it.
  sync,

  /// The commit returns without waiting for the disk. Readers see it at
  /// once, a crash of the process loses none of it, and it becomes durable
  /// at the next sync commit, at [Database.sync], when the database closes,
  /// or within a second.
  deferred,
}

/// What hashing a password costs: Argon2id memory in KiB, iterations and
/// lanes. The default is 19 MiB, 2 iterations and 1 lane.
final class PasswordHashing {
  const PasswordHashing({
    required this.memoryKib,
    required this.iterations,
    required this.parallelism,
  });

  final int memoryKib;
  final int iterations;
  final int parallelism;
}

/// A migration step from the version before [version] to [version]: the
/// renames and deletions the engine cannot work out alone, and a function
/// that moves data.
final class Migration {
  const Migration(
    this.version, {
    this.renameCollections = const {},
    this.renameFields = const {},
    this.deleteCollections = const [],
    this.replaceFields = const {},
    this.run,
  });

  final int version;

  /// New names by old name.
  final Map<String, String> renameCollections;

  /// For each collection, by its name before the migration, new field names
  /// by old name.
  final Map<String, Map<String, String>> renameFields;

  /// Collections this step deletes, after its function has run.
  final List<String> deleteCollections;

  /// For each collection, by its name before the migration, the fields
  /// whose type changes: a field removed and a field added with the same
  /// name.
  final Map<String, List<String>> replaceFields;

  /// Moves data across, in the migration's write transaction. Through
  /// [Database.openAsync], it may be asynchronous; the calls it makes on the
  /// context are synchronous either way.
  final FutureOr<void> Function(MigrationContext context)? run;
}

/// What a migration function gets: the collections of the new schema, and
/// the objects as the schema before the migration reads them.
final class MigrationContext {
  MigrationContext._(this._txn, this._previous);

  final WriteTransaction _txn;
  final StoredSchema _previous;

  /// The schema version the file held before the migration.
  int get previousVersion => _previous.version;

  /// The collection of [schema] in the new schema.
  WriteCollection<T, Q, K> collection<
    T,
    Q extends QueryBuilder<T>,
    K extends Object
  >(CollectionSchema<T, Q, K> schema) => _txn.collection(schema);

  /// The primary keys of every object of collection [name], as the schema
  /// before the migration named it, in key order.
  List<Object> previousKeys(String name) {
    final handle = _txn._live();
    final names = _NameHandle(name);

    try {
      _check(darudb_migration_previous_keys(handle, names.pointer, _io.out));

      final reader = Reader(_io.outBytes());
      final keys = <Object>[];

      while (!reader.isDone) {
        keys.add(reader.any()!);
      }

      return keys;
    } finally {
      names.free();
    }
  }

  /// The object of collection [name] whose primary key is [key], as the
  /// schema before the migration reads it: its fields by name, with those of
  /// an embedded object in a map of their own. `null` when there is none.
  Map<String, Object?>? previous(String name, Object key) {
    final handle = _txn._live();
    final names = _NameHandle(name);
    final fields = _previous.collections[name];

    if (fields == null) {
      names.free();

      throw invalidArgument('the schema before the migration has no `$name`');
    }

    try {
      final writer = _io.writer..reset();

      writeKey(writer, key);

      final keyBytes = _io.load(writer.written);
      final found = _check(
        darudb_migration_previous_record(
          handle,
          names.pointer,
          keyBytes,
          writer.at,
          _io.out,
        ),
      );

      if (found == 0) {
        return null;
      }

      return _named(Reader(_io.outBytes()).anyFields(), fields);
    } finally {
      names.free();
    }
  }

  static Map<String, Object?> _named(
    Map<int, Object?> record,
    List<StoredField> fields,
  ) => {
    for (final field in fields)
      field.name: switch (record[field.id]) {
        final Map<int, Object?> embedded when field.fields != null => _named(
          embedded,
          field.fields!,
        ),
        final RawLink link => link.key,
        null => field.defaultValue?.value,
        final value => value,
      },
  };
}

/// The memory calls into the engine pass bytes through: what goes in is
/// copied into native memory the library reads, and what comes out is read
/// where the library left it. One for the isolate, since a call returns
/// before the next starts.
final class _Io {
  final Writer writer = Writer(1024);
  final Pointer<Buf> out = calloc<Buf>();
  final Pointer<Buf> errorCode = calloc<Buf>();
  final Pointer<Buf> errorMessage = calloc<Buf>();
  final Pointer<Info> info = calloc<Info>();
  final Pointer<Uint64> number = calloc<Uint64>();
  final Pointer<Pointer<NativeDatabase>> database =
      calloc<Pointer<NativeDatabase>>();
  final Pointer<Pointer<NativeTransaction>> transaction =
      calloc<Pointer<NativeTransaction>>();
  final Pointer<Pointer<NativePrepared>> prepared =
      calloc<Pointer<NativePrepared>>();

  Pointer<Uint8> _in = nullptr;
  Uint8List _view = Uint8List(0);

  /// Copies [bytes], and any [more] after them, into native memory, and
  /// returns where they start.
  Pointer<Uint8> load(Uint8List bytes, [Uint8List? more]) {
    final total = bytes.length + (more?.length ?? 0);

    if (total > _view.length) {
      var size = _view.isEmpty ? 4096 : _view.length;

      while (size < total) {
        size *= 2;
      }

      if (_in != nullptr) {
        malloc.free(_in);
      }

      _in = malloc<Uint8>(size);
      _view = _in.asTypedList(size);
    }

    _view.setRange(0, bytes.length, bytes);

    if (more != null) {
      _view.setRange(bytes.length, total, more);
    }

    return _in;
  }

  /// Fills what [load] copied with zeros, for a key or a password.
  void wipe(int length) {
    _view.fillRange(0, length.clamp(0, _view.length), 0);
  }

  /// The bytes the last call handed out, read where they lie until the
  /// next call.
  Uint8List outBytes() {
    final ref = out.ref;

    return ref.len == 0 ? Uint8List(0) : ref.ptr.asTypedList(ref.len);
  }
}

final _Io _io = _Io();

/// [status], unless it says the call failed, in which case the call's error.
int _check(int status) {
  if (status >= 0) {
    return status;
  }

  darudb_last_error(_io.errorCode, _io.errorMessage);

  String text(Pointer<Buf> buf) {
    final ref = buf.ref;

    return ref.len == 0
        ? ''
        : utf8.decode(ref.ptr.asTypedList(ref.len), allowMalformed: true);
  }

  throw DaruException(text(_io.errorCode), text(_io.errorMessage));
}

/// A collection's name, made once in the library for the calls that use it.
final class _NameHandle {
  _NameHandle(String name) {
    final bytes = utf8.encode(name);

    pointer = darudb_name(_io.load(bytes), bytes.length);
  }

  late final Pointer<NativeName> pointer;

  void free() => darudb_name_free(pointer);
}

final _databaseFinalizer = NativeFinalizer(
  Native.addressOf<NativeFunction<Void Function(Pointer<NativeDatabase>)>>(
    darudb_database_free,
  ).cast(),
);

final _nameFinalizer = NativeFinalizer(
  Native.addressOf<NativeFunction<Void Function(Pointer<NativeName>)>>(
    darudb_name_free,
  ).cast(),
);

final _preparedFinalizer = NativeFinalizer(
  Native.addressOf<NativeFunction<Void Function(Pointer<NativePrepared>)>>(
    darudb_prepared_free,
  ).cast(),
);

/// What a database knows of one of its collections: the schema the
/// application declared, how its fields lie in the file, and its name in the
/// library.
final class _Collection {
  _Collection(this.schema, this.layout, this.name);

  final CollectionSchema<Object?, QueryBuilder<Object?>, Object> schema;
  final Layout layout;
  final Pointer<NativeName> name;
  late final List<String> names = [
    for (final field in schema.fields) field.name,
  ];
}

/// The options of `Database.open` as the record the library reads. A key and
/// a password together are refused here: the engine takes whichever comes
/// last, and which one opened the file would depend on the record's order.
Uint8List _options({
  required bool create,
  required int? pageSize,
  required Duration? busyTimeout,
  required int? cacheSize,
  required Uint8List? schema,
  required Uint8List? key,
  required String? password,
  required PasswordHashing? passwordHashing,
  required List<Migration> migrations,
  bool upgradeFormat = true,
}) {
  if (key != null && password != null) {
    throw invalidArgument('give a key or a password, not both');
  }

  final writer = Writer(256);
  // The password's bytes, which are wiped with the writer's once written.
  final secret = password == null ? null : utf8.encode(password);
  final entries = [
    (1, (Writer w) => w.byte(create ? Tag.trueValue : Tag.falseValue)),
    if (pageSize != null)
      (
        2,
        (Writer w) => w
          ..byte(Tag.int64)
          ..int64(pageSize),
      ),
    if (busyTimeout != null)
      (
        3,
        (Writer w) => w
          ..byte(Tag.int64)
          ..int64(busyTimeout.inMilliseconds),
      ),
    if (cacheSize != null)
      (
        4,
        (Writer w) => w
          ..byte(Tag.int64)
          ..int64(cacheSize),
      ),
    if (schema != null)
      (
        5,
        (Writer w) => w
          ..byte(Tag.bytes)
          ..bytesOf(schema),
      ),
    if (passwordHashing != null)
      (
        8,
        (Writer w) {
          final mark = w.open();

          w
            ..varint(3)
            ..varint(1)
            ..byte(Tag.int64)
            ..int64(passwordHashing.memoryKib)
            ..varint(2)
            ..byte(Tag.int64)
            ..int64(passwordHashing.iterations)
            ..varint(3)
            ..byte(Tag.int64)
            ..int64(passwordHashing.parallelism);
          w.close(mark);
        },
      ),
    if (!upgradeFormat) (10, (Writer w) => w.byte(Tag.falseValue)),
    if (migrations.isNotEmpty)
      (
        9,
        (Writer w) {
          w
            ..byte(Tag.list)
            ..varint(migrations.length);

          for (final migration in migrations) {
            _migration(w, migration);
          }
        },
      ),
    // The key or the password goes last. The writer grows by copying into a
    // larger buffer and dropping the old one, so a secret written before
    // something else could stay behind in a buffer nobody wipes; written
    // last, it is only ever in the buffer the `finally` below wipes.
    if (key != null)
      (
        6,
        (Writer w) => w
          ..byte(Tag.bytes)
          ..bytesOf(key),
      ),
    if (secret != null)
      (
        7,
        (Writer w) => w
          ..byte(Tag.bytes)
          ..bytesOf(secret),
      ),
  ];

  try {
    writer.varint(entries.length);

    for (final (id, write) in entries) {
      writer.varint(id);
      write(writer);
    }

    return Uint8List.fromList(writer.written);
  } finally {
    writer.bytes.fillRange(0, writer.bytes.length, 0);
    secret?.fillRange(0, secret.length, 0);
  }
}

void _migration(Writer w, Migration migration) {
  void strings(List<String> values) {
    final mark = w.open();

    w.varint(values.length);

    for (final (index, value) in values.indexed) {
      w
        ..varint(index + 1)
        ..byte(Tag.string)
        ..string(value);
    }

    w.close(mark);
  }

  final renamedFields = [
    for (final MapEntry(key: collection, value: renames)
        in migration.renameFields.entries)
      for (final MapEntry(key: from, value: to) in renames.entries)
        [collection, from, to],
  ];
  final replaced = [
    for (final MapEntry(key: collection, value: fields)
        in migration.replaceFields.entries)
      for (final field in fields) [collection, field],
  ];
  final mark = w.open();

  w
    ..varint(5)
    ..varint(1)
    ..byte(Tag.int64)
    ..int64(migration.version)
    ..varint(2)
    ..byte(Tag.list)
    ..varint(migration.renameCollections.length);

  for (final MapEntry(key: from, value: to)
      in migration.renameCollections.entries) {
    strings([from, to]);
  }

  w
    ..varint(3)
    ..byte(Tag.list)
    ..varint(renamedFields.length);

  renamedFields.forEach(strings);
  w
    ..varint(4)
    ..byte(Tag.list)
    ..varint(migration.deleteCollections.length);

  for (final name in migration.deleteCollections) {
    w
      ..byte(Tag.string)
      ..string(name);
  }

  w
    ..varint(5)
    ..byte(Tag.list)
    ..varint(replaced.length);

  replaced.forEach(strings);
  w.close(mark);
}

/// The write transaction of a migration under way, with the collections of
/// the schema it leads to, and the context its functions get.
(WriteTransaction, MigrationContext) _beginMigration(
  Pointer<NativeTransaction> handle,
  Schema schema,
) {
  final txn = WriteTransaction._(handle, null);

  try {
    _check(darudb_migration_schema_record(handle, 0, _io.out));

    final next = StoredSchema.decode(Uint8List.fromList(_io.outBytes()));

    _check(darudb_migration_schema_record(handle, 1, _io.out));

    final previous = StoredSchema.decode(Uint8List.fromList(_io.outBytes()));

    txn._collections = {
      for (final collection in schema.collections)
        collection: _Collection(
          collection,
          next.layoutOf(collection),
          _NameHandle(collection.name).pointer,
        ),
    };

    return (txn, MigrationContext._(txn, previous));
  } on Object {
    _endMigration(txn);

    rethrow;
  }
}

/// Ends a migration's transaction, throwing it away if it has not finished.
void _endMigration(WriteTransaction txn) {
  for (final collection in txn._collections?.values ?? const <_Collection>[]) {
    darudb_name_free(collection.name);
  }

  txn._collections = null;
  txn._end();
}

/// An open database. There is no constructor: use [Database.open].
final class Database implements Finalizable {
  Database._(this.path, this._handle, Schema? schema) : _file = _fileOf(path) {
    _databaseFinalizer.attach(this, _handle.cast(), detach: this);

    final record = _check(darudb_schema_record(_handle, _io.out)) == 1
        ? Uint8List.fromList(_io.outBytes())
        : null;

    if (record != null) {
      final stored = StoredSchema.decode(record);

      _schemaVersion = stored.version;

      for (final collection in schema?.collections ?? const <Never>[]) {
        final name = _NameHandle(collection.name).pointer;

        _nameFinalizer.attach(this, name.cast(), detach: this);
        _collections[collection] = _Collection(
          collection,
          stored.layoutOf(collection),
          name,
        );
      }
    }
  }

  /// Opens the database at [path], creating the file if it does not exist
  /// and [create] allows it.
  ///
  /// With a [schema], the file holds its collections: the first open stores
  /// it, and a file that holds an older version is migrated in one write
  /// transaction, through the [migrations] registered for the versions in
  /// between. A [key] of 32 bytes or a [password] encrypts a new database,
  /// and opens an encrypted one.
  ///
  /// A file in an older format version is raised to the newest this
  /// package writes, while no other process has it open, unless
  /// `upgradeFormat` is `false`. Raising it rewrites the header only; the
  /// older leaves take the newer, smaller layout as writes change them, and
  /// [compact] rewrites the trees where that saves room. A release that knows
  /// only the older version cannot open the file afterwards, so an app that
  /// may go back to one passes `false`, and calls `Database.upgradeFormat`
  /// once it no longer may; a new database is then created in format version
  /// 5, which every release reads.
  static Database open(
    String path, {
    Schema? schema,
    List<Migration> migrations = const [],
    bool create = true,
    int? pageSize,
    Duration? busyTimeout,
    int? cacheSize,
    Uint8List? key,
    String? password,
    PasswordHashing? passwordHashing,
    bool upgradeFormat = true,
  }) {
    final options = _options(
      create: create,
      pageSize: pageSize,
      busyTimeout: busyTimeout,
      cacheSize: cacheSize,
      schema: schema == null ? null : encodeSchema(schema),
      key: key,
      password: password,
      passwordHashing: passwordHashing,
      migrations: migrations,
      upgradeFormat: upgradeFormat,
    );
    final pathBytes = utf8.encode(path);
    final loaded = _io.load(pathBytes, options);
    final int status;

    try {
      status = _check(
        darudb_open(
          loaded,
          pathBytes.length,
          loaded + pathBytes.length,
          options.length,
          _io.database,
          _io.transaction,
        ),
      );
    } finally {
      // The options may hold a key or a password, which the library has
      // copied by now.
      _io.wipe(pathBytes.length + options.length);
      options.fillRange(0, options.length, 0);
    }

    if (status == 0) {
      return Database._(path, _io.database.value, schema);
    }

    return _migrate(path, _io.transaction.value, schema!, migrations);
  }

  /// Runs a migration under way: each version step's function, in version
  /// order, then the engine's finish, which commits it.
  static Database _migrate(
    String path,
    Pointer<NativeTransaction> handle,
    Schema schema,
    List<Migration> migrations,
  ) {
    final (txn, context) = _beginMigration(handle, schema);

    try {
      while (_check(darudb_migration_next_step(handle, _io.number)) == 1) {
        final version = _io.number.value;

        for (final migration in migrations) {
          if (migration.version == version) {
            final result = migration.run?.call(context);

            if (result is Future) {
              result.ignore();

              throw invalidArgument(
                'a migration function of `Database.open` returned a Future; '
                'open with `Database.openAsync` for one that is asynchronous',
              );
            }
          }
        }
      }

      _check(darudb_migration_finish(handle, _io.database));

      return Database._(path, _io.database.value, schema);
    } finally {
      _endMigration(txn);
    }
  }

  /// Opens the database at [path] as [open] does, on a thread of the native
  /// library, so that the isolate does not wait for the file, a recovery or
  /// a migration's commit. A migration function may be asynchronous.
  static Future<Database> openAsync(
    String path, {
    Schema? schema,
    List<Migration> migrations = const [],
    bool create = true,
    int? pageSize,
    Duration? busyTimeout,
    int? cacheSize,
    Uint8List? key,
    String? password,
    PasswordHashing? passwordHashing,
    bool upgradeFormat = true,
  }) async => _openAsync(
    path,
    schema,
    migrations,
    _options(
      create: create,
      pageSize: pageSize,
      busyTimeout: busyTimeout,
      cacheSize: cacheSize,
      schema: schema == null ? null : encodeSchema(schema),
      key: key,
      password: password,
      passwordHashing: passwordHashing,
      migrations: migrations,
      upgradeFormat: upgradeFormat,
    ),
  );

  /// The path the database was opened at.
  final String path;

  /// The file, as this isolate's asynchronous writes are queued on it: the
  /// path with its links resolved, so that two paths to one file share a
  /// queue.
  final String _file;

  static String _fileOf(String path) {
    try {
      return File(path).resolveSymbolicLinksSync();
    } on Object {
      return File(path).absolute.path;
    }
  }

  /// The files with a synchronous write transaction under way in this
  /// isolate, whose function is running.
  static final Set<String> _writing = {};

  /// Refuses a synchronous call that would wait for this isolate's own
  /// write: the function of a synchronous one that is running, or an
  /// asynchronous one, which needs the isolate's event loop to finish.
  void _refuseWhileWritingAsync(String call) {
    if (_writing.contains(_file)) {
      throw invalidArgument(
        'write transactions do not nest: inside a write function, a `$call` '
        'on the same file would wait for the function itself',
      );
    }

    if (_turns.isHeld(_file)) {
      throw invalidArgument(
        'a synchronous `$call` waits for the writer, which is an '
        'asynchronous write of this isolate that needs its event loop; use '
        '`${call}Async`',
      );
    }
  }

  Pointer<NativeDatabase> _handle;
  int? _schemaVersion;
  final Map<Object, _Collection> _collections = Map.identity();
  final Map<String, Prepared<Object?>> _kept = {};

  /// Whether [close] has not been called.
  bool get isOpen => _handle != nullptr;

  Pointer<NativeDatabase> _live() {
    if (_handle == nullptr) {
      throw const DaruException('CLOSED', 'the database is closed');
    }

    return _handle;
  }

  Info _info() {
    _check(darudb_database_info(_live(), _io.info));

    return _io.info.ref;
  }

  /// The size of every page in the file, in bytes.
  int get pageSize => _info().pageSize;

  /// The file format version recorded in the file: 6, which this package
  /// writes, or 5 for a file that opening did not raise.
  int get formatVersion => _info().formatVersion;

  /// Whether the file is encrypted.
  bool get isEncrypted => _info().encrypted != 0;

  /// The schema version the file holds, or `null` without a schema.
  int? get schemaVersion => _schemaVersion;

  /// Runs [fn] in a read transaction and returns what it returns. The
  /// transaction sees one commit for as long as it lives.
  R read<R>(R Function(ReadTransaction txn) fn) {
    _check(darudb_begin_read(_live(), _io.transaction));

    final txn = ReadTransaction._(_io.transaction.value, this);

    try {
      return _settled(fn(txn));
    } finally {
      txn._end();
    }
  }

  /// Runs [fn] in a write transaction, commits it when [fn] returns, aborts
  /// it when [fn] throws, and returns what [fn] returns.
  R write<R>(
    R Function(WriteTransaction txn) fn, {
    Durability durability = Durability.sync,
  }) {
    _refuseWhileWritingAsync('write');
    _check(darudb_begin_write(_live(), _io.transaction));

    final txn = WriteTransaction._(_io.transaction.value, this);

    _writing.add(_file);

    try {
      final R result;

      try {
        result = _settled(fn(txn));
      } finally {
        _writing.remove(_file);
      }

      _check(
        darudb_commit(txn._live(), durability == Durability.deferred ? 1 : 0),
      );

      return result;
    } finally {
      txn._end();
    }
  }

  static R _settled<R>(R result) {
    if (result is Future) {
      // The transaction ends when the function returns, before the future
      // completes.
      result.ignore();

      throw invalidArgument(
        'a transaction function of the synchronous API returned a Future; '
        'it would run after the transaction ended',
      );
    }

    return result;
  }

  /// Runs [fn], which may be asynchronous, in a read transaction, and
  /// resolves to what it resolves to. The transaction's calls run on threads
  /// of the native library, one after another in the order they were made.
  Future<R> readAsync<R>(FutureOr<R> Function(AsyncReadTransaction txn) fn) =>
      _readAsync(this, fn);

  /// Runs [fn], which may be asynchronous, in a write transaction, commits
  /// it when [fn] resolves, aborts it when [fn] fails, and resolves to what
  /// [fn] resolves to. This isolate's asynchronous writes on one file take
  /// turns.
  Future<R> writeAsync<R>(
    FutureOr<R> Function(AsyncWriteTransaction txn) fn, {
    Durability durability = Durability.sync,
  }) => _writeAsync(this, fn, durability);

  /// [sync] on a thread of the native library, after this isolate's
  /// asynchronous writes on the file.
  Future<void> syncAsync() => _turns.inTurn(_file, () async {
    await _call((id, callback) => darudb_sync_async(_live(), id, callback));
  });

  /// [close] on a thread of the native library, after this isolate's
  /// asynchronous writes on the file.
  Future<void> closeAsync() => _turns.inTurn(_file, () async {
    final handle = _handle;

    if (handle == nullptr) {
      return;
    }

    try {
      await _call((id, callback) => darudb_close_async(handle, id, callback));
    } finally {
      _release(handle);
    }
  });

  /// [setKey] on a thread of the native library, after this isolate's
  /// asynchronous writes on the file: changing the key commits.
  Future<void> setKeyAsync(Uint8List key) {
    final copy = Uint8List.fromList(key);

    return _turns.inTurn(_file, () async {
      final loaded = _io.load(copy);
      final Future<_Reply> done;

      try {
        done = _call(
          (id, callback) =>
              darudb_set_key_async(_live(), loaded, copy.length, id, callback),
        );
      } finally {
        _io.wipe(copy.length);
        copy.fillRange(0, copy.length, 0);
      }

      await done;
    });
  }

  /// [setPassword] on a thread of the native library, after this
  /// isolate's asynchronous writes on the file: changing the password
  /// commits.
  Future<void> setPasswordAsync(String password) {
    final bytes = utf8.encode(password);

    return _turns.inTurn(_file, () async {
      final loaded = _io.load(bytes);
      final Future<_Reply> done;

      try {
        done = _call(
          (id, callback) => darudb_set_password_async(
            _live(),
            loaded,
            bytes.length,
            id,
            callback,
          ),
        );
      } finally {
        _io.wipe(bytes.length);
        bytes.fillRange(0, bytes.length, 0);
      }

      await done;
    });
  }

  /// Checks the published commit completely: every page against its check,
  /// the order of every key, every count, that every page is used, free or
  /// retained exactly once, and every object against its indexes. It reports
  /// every problem it finds rather than throwing, and reads while other
  /// handles and processes write.
  CheckReport check() {
    _check(darudb_check(_live(), _io.out));

    return CheckReport._(_report(_io.outBytes()));
  }

  /// [check] on a thread of the native library.
  Future<CheckReport> checkAsync() async {
    final reply = await _call(
      (id, callback) => darudb_check_async(_live(), id, callback),
    );

    return CheckReport._(_report(reply.bytes));
  }

  /// Writes a copy of the published commit to a new file at [path], while
  /// other handles and processes may write. The copy holds no free space,
  /// has the file's page size, and opens with the same key or password. A
  /// path that is taken fails with `INVALID_ARGUMENT`.
  ///
  /// A [key] or a [password] encrypts the copy under a new random data key,
  /// which it wraps, at the cost [passwordHashing] sets for a password.
  /// Changing a file's key or password only wraps its data key again, so a
  /// backup is the way to leave behind a data key that may have been
  /// exposed. A plain database's copy is encrypted the same way. The package
  /// copies the key or password when the call is made.
  BackupReport backup(
    String path, {
    Uint8List? key,
    String? password,
    PasswordHashing? passwordHashing,
  }) {
    final options = _backupOptions(key, password, passwordHashing);
    final bytes = utf8.encode(path);
    final loaded = _io.load(bytes, options);

    try {
      _check(
        darudb_backup(
          _live(),
          loaded,
          bytes.length,
          loaded + bytes.length,
          options.length,
          _io.out,
        ),
      );
    } finally {
      _io.wipe(bytes.length + options.length);
      options.fillRange(0, options.length, 0);
    }

    return BackupReport._(_report(_io.outBytes()));
  }

  /// [backup] on a thread of the native library.
  Future<BackupReport> backupAsync(
    String path, {
    Uint8List? key,
    String? password,
    PasswordHashing? passwordHashing,
  }) async {
    final options = _backupOptions(key, password, passwordHashing);
    final bytes = utf8.encode(path);
    final loaded = _io.load(bytes, options);
    final Future<_Reply> backedUp;

    try {
      backedUp = _call(
        (id, callback) => darudb_backup_async(
          _live(),
          loaded,
          bytes.length,
          loaded + bytes.length,
          options.length,
          id,
          callback,
        ),
      );
    } finally {
      _io.wipe(bytes.length + options.length);
      options.fillRange(0, options.length, 0);
    }

    return BackupReport._(_report((await backedUp).bytes));
  }

  /// Makes the file smaller in place, writing again, full, the trees whose
  /// pages inserts left part empty, and moving the pages at its end into
  /// free pages nearer its start, while other handles and processes go on
  /// using it.
  CompactReport compact() {
    _refuseWhileWritingAsync('compact');
    _check(darudb_compact(_live(), _io.out));

    return CompactReport._(_report(_io.outBytes()));
  }

  /// [compact] on a thread of the native library, after this isolate's
  /// asynchronous writes on the file.
  Future<CompactReport> compactAsync() => _turns.inTurn(_file, () async {
    final reply = await _call(
      (id, callback) => darudb_compact_async(_live(), id, callback),
    );

    return CompactReport._(_report(reply.bytes));
  });

  /// Rescues what it can of the damaged file at [from] into a new file at
  /// [into], reading it page by page, so it works on a file that does not
  /// open. A [key] or a [password] reads an encrypted file, and the new file
  /// is encrypted under the same key. It waits up to [busyTimeout] for other
  /// processes to close the file.
  static SalvageReport salvage(
    String from,
    String into, {
    Duration? busyTimeout,
    Uint8List? key,
    String? password,
  }) {
    final options = _salvageOptions(busyTimeout, key, password);
    final fromBytes = utf8.encode(from);
    final intoBytes = utf8.encode(into);
    final paths = Uint8List.fromList([...fromBytes, ...intoBytes]);
    final loaded = _io.load(paths, options);
    final int whole;

    try {
      whole = _check(
        darudb_salvage(
          loaded,
          fromBytes.length,
          loaded + fromBytes.length,
          intoBytes.length,
          loaded + paths.length,
          options.length,
          _io.out,
        ),
      );
    } finally {
      _io.wipe(paths.length + options.length);
      options.fillRange(0, options.length, 0);
    }

    return SalvageReport._(_report(_io.outBytes()), whole == 1);
  }

  /// [salvage] on a thread of the native library.
  static Future<SalvageReport> salvageAsync(
    String from,
    String into, {
    Duration? busyTimeout,
    Uint8List? key,
    String? password,
  }) async {
    final options = _salvageOptions(busyTimeout, key, password);
    final fromBytes = utf8.encode(from);
    final intoBytes = utf8.encode(into);
    final paths = Uint8List.fromList([...fromBytes, ...intoBytes]);
    final loaded = _io.load(paths, options);
    final Future<_Reply> salvaged;

    try {
      salvaged = _call(
        (id, callback) => darudb_salvage_async(
          loaded,
          fromBytes.length,
          loaded + fromBytes.length,
          intoBytes.length,
          loaded + paths.length,
          options.length,
          id,
          callback,
        ),
      );
    } finally {
      _io.wipe(paths.length + options.length);
      options.fillRange(0, options.length, 0);
    }

    final reply = await salvaged;

    return SalvageReport._(_report(reply.bytes), reply.status == 1);
  }

  /// Prepares a query in the query language on [collection], parsed once
  /// here, with `$0`, `$1` and on for the values each run gives.
  Prepared<T> prepare<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> collection,
    String text,
  ) {
    _live();

    final name = utf8.encode(collection.name);
    final query = utf8.encode(text);
    final loaded = _io.load(name, query);

    _check(
      darudb_prepare_text(
        loaded,
        name.length,
        loaded + name.length,
        query.length,
        _io.prepared,
      ),
    );

    return Prepared<T>._(_io.prepared.value, collection);
  }

  /// The query [text] on [collection], prepared once and kept for the next
  /// run of the same text.
  Prepared<Object?> _keptQuery(
    CollectionSchema<Object?, QueryBuilder<Object?>, Object> collection,
    String text,
  ) {
    final key = '${collection.name}\u0000$text';
    final kept = _kept[key];

    if (kept != null) {
      return kept;
    }

    if (_kept.length >= 256) {
      _kept.clear();
    }

    return _kept[key] = prepare(collection, text);
  }

  /// Makes every deferred commit durable, whichever handle or process made
  /// it.
  void sync() {
    _refuseWhileWritingAsync('sync');
    _check(darudb_sync(_live()));
  }

  /// Raises the file's format version to the newest this package writes, as
  /// opening does unless `upgradeFormat` is `false`, and returns whether it
  /// did: `false` for a file in that version already. It waits for the
  /// writer lock, and needs the file to itself: while another process has
  /// it open, it fails with `BUSY`. A release that knows only the older
  /// version cannot open the file afterwards.
  bool upgradeFormat() {
    _refuseWhileWritingAsync('upgradeFormat');

    return _check(darudb_upgrade_format(_live())) == 1;
  }

  /// [upgradeFormat] on a thread of the native library, after this isolate's
  /// asynchronous writes on the file.
  Future<bool> upgradeFormatAsync() => _turns.inTurn(_file, () async {
    final reply = await _call(
      (id, callback) => darudb_upgrade_format_async(_live(), id, callback),
    );

    return reply.status == 1;
  });

  /// Changes the key of an encrypted database to [key], 32 bytes. No page
  /// is encrypted again.
  void setKey(Uint8List key) {
    _refuseWhileWritingAsync('setKey');

    final loaded = _io.load(key);

    try {
      _check(darudb_set_key(_live(), loaded, key.length));
    } finally {
      _io.wipe(key.length);
    }
  }

  /// Changes the password of an encrypted database to [password].
  void setPassword(String password) {
    _refuseWhileWritingAsync('setPassword');

    final bytes = utf8.encode(password);
    final loaded = _io.load(bytes);

    try {
      _check(darudb_set_password(_live(), loaded, bytes.length));
    } finally {
      _io.wipe(bytes.length);
      bytes.fillRange(0, bytes.length, 0);
    }
  }

  /// Closes the database, making deferred commits durable first. Every
  /// later call fails with `CLOSED`; closing again does nothing.
  void close() {
    final handle = _handle;

    if (handle == nullptr) {
      return;
    }

    _refuseWhileWritingAsync('close');

    try {
      _check(darudb_close(handle));
    } finally {
      _release(handle);
    }
  }

  /// Frees the handle and everything made for it, once it has closed.
  void _release(Pointer<NativeDatabase> handle) {
    _handle = nullptr;
    _databaseFinalizer.detach(this);
    _nameFinalizer.detach(this);
    darudb_database_free(handle);

    for (final collection in _collections.values) {
      darudb_name_free(collection.name);
    }

    _collections.clear();
    _kept.clear();
  }

  _Collection _collection(Object schema) {
    final found = _collections[schema];

    if (found == null) {
      _live();

      throw invalidArgument(
        'the database was opened without this collection in its schema',
      );
    }

    return found;
  }
}

/// A query in the query language, parsed once, which each run gives values
/// for its parameters.
final class Prepared<T> implements Finalizable {
  Prepared._(this._handle, this._collection) {
    _preparedFinalizer.attach(this, _handle.cast(), detach: this);
  }

  final Pointer<NativePrepared> _handle;
  final CollectionSchema<Object?, QueryBuilder<Object?>, Object> _collection;
}

/// A read transaction: one commit, as it was when the transaction began.
base class ReadTransaction {
  ReadTransaction._(this._handle, this._database);

  Pointer<NativeTransaction> _handle;
  final Database? _database;
  Map<Object, _Collection>? _collections;

  Pointer<NativeTransaction> _live() {
    if (_handle == nullptr) {
      throw const DaruException(
        'CLOSED',
        'the transaction has ended: a transaction and its collections are '
            'used inside its function',
      );
    }

    return _handle;
  }

  void _end() {
    final handle = _handle;

    if (handle != nullptr) {
      _handle = nullptr;
      darudb_txn_free(handle);
    }
  }

  _Collection _collection(Object schema) {
    final found = _collections?[schema];

    if (found != null) {
      return found;
    }

    final database = _database;

    if (database == null) {
      throw invalidArgument('the migration has no such collection');
    }

    return database._collection(schema);
  }

  /// The collection of [schema], for reading its objects.
  ReadCollection<T, Q, K>
  collection<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> schema,
  ) => ReadCollection<T, Q, K>._(this, schema, _collection(schema));
}

/// A write transaction: what a read transaction offers, with this
/// transaction's changes, and the calls that change objects.
final class WriteTransaction extends ReadTransaction {
  WriteTransaction._(super._handle, super._database) : super._();

  /// The collection of [schema], for reading and writing its objects.
  @override
  WriteCollection<T, Q, K>
  collection<T, Q extends QueryBuilder<T>, K extends Object>(
    CollectionSchema<T, Q, K> schema,
  ) => WriteCollection<T, Q, K>._(this, schema, _collection(schema));
}

/// A collection of a transaction, for reading its objects.
base class ReadCollection<T, Q extends QueryBuilder<T>, K extends Object> {
  ReadCollection._(this._txn, this._schema, this._known);

  final ReadTransaction _txn;
  final CollectionSchema<T, Q, K> _schema;
  final _Collection _known;

  /// The collection's name.
  String get name => _schema.name;

  /// The object whose primary key is [key], or `null`.
  T? get(K key) {
    final handle = _txn._live();
    final writer = _io.writer..reset();

    writeKey(writer, key);

    final found = _check(
      darudb_get(
        handle,
        _known.name,
        _io.load(writer.written),
        writer.at,
        _io.out,
      ),
    );

    if (found == 0) {
      return null;
    }

    final bytes = _io.outBytes();

    return _schema.decode(bytes, 0, bytes.length, _known.layout);
  }

  Q _built(QueryBuilder<T> Function(Q q)? query) {
    final builder = _schema.newQuery();

    query?.call(builder);

    return builder;
  }

  List<T> _records(Uint8List bytes, {int most = -1}) {
    final reader = Reader(bytes);
    final found = <T>[];

    while (!reader.isDone && found.length != most) {
      final length = reader.varint();
      final start = reader.at;

      reader.at += length;
      found.add(_schema.decode(bytes, start, start + length, _known.layout));
    }

    return found;
  }

  List<T> _find(QueryBuilder<T> Function(Q q)? query, {required bool first}) {
    final handle = _txn._live();
    final writer = _io.writer..reset();

    encodeQuery(writer, _schema.name, _built(query));
    _check(
      darudb_find(
        handle,
        _io.load(writer.written),
        writer.at,
        first ? 1 : 0,
        _io.out,
      ),
    );

    return _records(_io.outBytes());
  }

  /// The objects the query finds, in its order: every object, in primary
  /// key order, without one.
  ///
  /// ```dart
  /// users.find((q) => q.where(q.age.atLeast(18)).sortBy(q.name).limit(10));
  /// ```
  List<T> find([QueryBuilder<T> Function(Q q)? query]) =>
      _find(query, first: false);

  /// The first object the query finds, or `null`. The engine stops reading
  /// there.
  T? findOne([QueryBuilder<T> Function(Q q)? query]) {
    final found = _find(query, first: true);

    return found.isEmpty ? null : found.first;
  }

  /// How many objects the query finds, after its offset and within its
  /// limit: every object without one.
  int count([QueryBuilder<T> Function(Q q)? query]) {
    final handle = _txn._live();
    final writer = _io.writer..reset();

    encodeQuery(writer, _schema.name, _built(query));
    _check(
      darudb_count(handle, _io.load(writer.written), writer.at, _io.number),
    );

    return _io.number.value;
  }

  Prepared<Object?> _preparedOf(Prepared<T> prepared) {
    if (prepared._collection.name != _schema.name) {
      throw invalidArgument(
        'a query prepared on `${prepared._collection.name}` runs on that '
        'collection, not on `${_schema.name}`',
      );
    }

    return prepared;
  }

  List<T> _findPrepared(
    Prepared<Object?> prepared,
    List<Object?> parameters, {
    required bool first,
  }) {
    final handle = _txn._live();
    final writer = _io.writer..reset();

    encodeParameters(writer, parameters);
    _check(
      darudb_find_prepared(
        handle,
        prepared._handle,
        _io.load(writer.written),
        writer.at,
        first ? 1 : 0,
        _io.out,
      ),
    );

    return _records(_io.outBytes());
  }

  int _countPrepared(Prepared<Object?> prepared, List<Object?> parameters) {
    final handle = _txn._live();
    final writer = _io.writer..reset();

    encodeParameters(writer, parameters);
    _check(
      darudb_count_prepared(
        handle,
        prepared._handle,
        _io.load(writer.written),
        writer.at,
        _io.number,
      ),
    );

    return _io.number.value;
  }

  Prepared<Object?> _kept(String text) {
    final database = _txn._database;

    if (database == null) {
      throw invalidArgument('a migration runs queries built with a builder');
    }

    return database._keptQuery(_known.schema, text);
  }

  /// The objects the query in the query language [text] finds, with
  /// [parameters] for `$0`, `$1` and on. The text is parsed once and kept
  /// for the next run of the same text.
  ///
  /// ```dart
  /// users.findText(r'age >= $0 SORT BY name LIMIT 10', [18]);
  /// ```
  List<T> findText(String text, [List<Object?> parameters = const []]) =>
      _findPrepared(_kept(text), parameters, first: false);

  /// The first object [text] finds, or `null`.
  T? findOneText(String text, [List<Object?> parameters = const []]) {
    final found = _findPrepared(_kept(text), parameters, first: true);

    return found.isEmpty ? null : found.first;
  }

  /// How many objects [text] finds.
  int countText(String text, [List<Object?> parameters = const []]) =>
      _countPrepared(_kept(text), parameters);

  /// The objects a query [Database.prepare] prepared finds, with
  /// [parameters] for its parameters.
  List<T> findPrepared(
    Prepared<T> prepared, [
    List<Object?> parameters = const [],
  ]) => _findPrepared(_preparedOf(prepared), parameters, first: false);

  /// The first object a prepared query finds, or `null`.
  T? findOnePrepared(
    Prepared<T> prepared, [
    List<Object?> parameters = const [],
  ]) {
    final found = _findPrepared(_preparedOf(prepared), parameters, first: true);

    return found.isEmpty ? null : found.first;
  }

  /// How many objects a prepared query finds.
  int countPrepared(
    Prepared<T> prepared, [
    List<Object?> parameters = const [],
  ]) => _countPrepared(_preparedOf(prepared), parameters);
}

/// A collection of a write transaction, for reading and writing its
/// objects.
final class WriteCollection<T, Q extends QueryBuilder<T>, K extends Object>
    extends ReadCollection<T, Q, K> {
  WriteCollection._(super._txn, super._schema, super._known) : super._();

  final FieldSink _sink = FieldSink();

  List<K> _write(Iterable<T> objects, {required bool replace}) {
    final handle = _txn._live();
    final writer = _io.writer..reset();

    for (final object in objects) {
      final mark = writer.openLength();

      _schema.encode(object, writer, _known.layout, _sink);
      writer.close(mark);
    }

    _check(
      darudb_write(
        handle,
        _known.name,
        _io.load(writer.written),
        writer.at,
        replace ? 1 : 0,
        _io.out,
      ),
    );

    final reader = Reader(_io.outBytes());
    final keys = <K>[];

    while (!reader.isDone) {
      keys.add(_schema.keyOf(reader.any()));
    }

    return keys;
  }

  /// Inserts [object] and returns its primary key. In a collection keyed by
  /// an auto-increment, an object whose `id` is `null` gets the next number.
  /// It fails with `DUPLICATE_KEY` if the key is taken, or if a unique index
  /// finds one of the object's values taken, and leaves the transaction as
  /// it was.
  K insert(T object) => _write([object], replace: false).single;

  /// Inserts [objects], in one call into the engine, and returns their keys.
  /// A refused object stops the batch with its error, and the objects before
  /// it stay inserted in the transaction.
  List<K> insertMany(Iterable<T> objects) => _write(objects, replace: false);

  /// Inserts [object], or replaces the object with its primary key, and
  /// returns the key.
  K put(T object) => _write([object], replace: true).single;

  /// Inserts or replaces [objects]; see [insertMany].
  List<K> putMany(Iterable<T> objects) => _write(objects, replace: true);

  /// Deletes the object whose primary key is [key], and returns whether
  /// there was one.
  bool delete(K key) {
    final handle = _txn._live();
    final writer = _io.writer..reset();

    writeKey(writer, key);

    return _check(
          darudb_delete(
            handle,
            _known.name,
            _io.load(writer.written),
            writer.at,
          ),
        ) ==
        1;
  }

  /// Sets the fields [changes] gives in the object whose primary key is
  /// [key], keeps the rest, and returns whether there was one: it inserts
  /// nothing when there is none. Setting null makes an optional field null
  /// and gives a field with a default its default.
  ///
  /// ```dart
  /// users.update(id, (q) => [q.age.set(37), q.email.set(null)]);
  /// ```
  bool update(K key, List<Change> Function(Q q) changes) {
    final handle = _txn._live();
    final keyWriter = Writer(32);

    writeKey(keyWriter, key);

    final writer = _io.writer..reset();

    encodeChanges(
      writer,
      changes(_schema.newQuery()),
      _known.layout,
      _known.names,
    );

    final loaded = _io.load(keyWriter.written, writer.written);

    return _check(
          darudb_update(
            handle,
            _known.name,
            loaded,
            keyWriter.at,
            loaded + keyWriter.at,
            writer.at,
          ),
        ) ==
        1;
  }
}

/// The version of the DaruDB engine inside this package.
String get engineVersion {
  darudb_engine_version(_io.out);

  return utf8.decode(_io.outBytes());
}

/// The file format version this build of the engine reads and writes.
int get formatVersion => darudb_format_version();
