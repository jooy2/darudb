part of 'database.dart';

// The `Future` API: what the synchronous API does, with the engine's work on
// threads of the native library, so that the isolate never waits for the
// disk or for another writer. A call hands its arguments to the library,
// which copies them before it returns, and the result comes back through a
// `NativeCallable.listener` on the isolate's event loop.
//
// The calls of one transaction run one after another, in the order they
// were made, whether or not each was awaited. This isolate's asynchronous
// writes on one file take turns, and a synchronous write, sync or close on
// the file waits for none of them: it is refused while one runs, since it
// would hold the isolate the running write needs to finish.

/// The status and bytes an asynchronous call ends with.
final class _Reply {
  _Reply(this.status, this.bytes);

  final int status;
  final Uint8List bytes;

  /// The handle the call made, from its address in the bytes.
  Pointer<T> pointer<T extends NativeType>() => Pointer<T>.fromAddress(
    ByteData.sublistView(bytes).getUint64(0, Endian.little),
  );

  /// The count the call made.
  int get count => ByteData.sublistView(bytes).getUint64(0, Endian.little);
}

final Map<int, Completer<_Reply>> _waiting = {};
int _nextCall = 0;
NativeCallable<ResultCallback>? _listener;

void _receive(int id, int status, Pointer<Uint8> data, int length) {
  final bytes = length == 0
      ? Uint8List(0)
      : Uint8List.fromList(data.asTypedList(length));

  if (length != 0) {
    darudb_buffer_free(data, length);
  }

  final completer = _waiting.remove(id);

  // The listener keeps the isolate alive only while a call is under way.
  if (_waiting.isEmpty) {
    _listener?.keepIsolateAlive = false;
  }

  if (completer == null) {
    return;
  }

  if (status >= 0) {
    completer.complete(_Reply(status, bytes));

    return;
  }

  final split = bytes.indexOf(0);

  completer.completeError(
    DaruException(
      utf8.decode(bytes.sublist(0, split < 0 ? bytes.length : split)),
      split < 0
          ? ''
          : utf8.decode(bytes.sublist(split + 1), allowMalformed: true),
    ),
  );
}

/// Makes an asynchronous call: [submit] hands the library its arguments with
/// an id and the callback, and the future completes with what the call
/// ends with.
Future<_Reply> _call(
  void Function(int id, Pointer<NativeFunction<ResultCallback>> callback)
  submit,
) {
  final listener = _listener ??= NativeCallable<ResultCallback>.listener(
    _receive,
  )..keepIsolateAlive = false;
  final id = _nextCall++;
  final completer = Completer<_Reply>();

  _waiting[id] = completer;
  listener.keepIsolateAlive = true;
  submit(id, listener.nativeFunction);

  return completer.future;
}

/// The zone value that names the files whose write transaction the code
/// running in the zone is inside, so that a write that would wait for its
/// own turn is refused rather than waiting for ever.
const Symbol _insideWrite = #darudbInsideWrite;

/// The asynchronous writes of this isolate, queued by file.
final class _Turns {
  final Map<String, (Future<void>, int)> _queues = {};

  /// Whether an asynchronous write, sync or close on [file] is under way or
  /// waiting.
  bool isHeld(String file) => _queues.containsKey(file);

  /// Runs [body] once every write queued on [file] before it has finished.
  /// Called from inside a write transaction's function on the same file, it
  /// fails with `INVALID_ARGUMENT`: that turn ends only when the function
  /// does.
  Future<T> inTurn<T>(String file, Future<T> Function() body) async {
    final inside = Zone.current[_insideWrite];

    if (inside is Set<String> && inside.contains(file)) {
      throw invalidArgument(
        'write transactions do not nest: inside a write function, a write, '
        'sync, close or compaction on the same file would wait for the '
        'function itself',
      );
    }

    final (previous, holders) = _queues[file] ?? (Future<void>.value(), 0);
    final done = Completer<void>();

    _queues[file] = (done.future, holders + 1);

    try {
      await previous;

      return await body();
    } finally {
      final (tail, left) = _queues[file]!;

      if (left == 1) {
        _queues.remove(file);
      } else {
        _queues[file] = (tail, left - 1);
      }

      done.complete();
    }
  }
}

final _Turns _turns = _Turns();

/// The calls of one transaction, in the order they were made.
final class _Serial {
  Future<void> _tail = Future<void>.value();

  /// Runs [operation] after every call made before it has finished.
  Future<T> run<T>(Future<T> Function() operation) {
    final previous = _tail;
    final done = Completer<void>();

    _tail = done.future;

    return previous.then((_) async {
      try {
        return await operation();
      } finally {
        done.complete();
      }
    });
  }

  /// Waits for every call made so far.
  Future<void> drain() => _tail;
}

/// Opens a database as [Database.open] does, on a thread of the library.
Future<Database> _openAsync(
  String path,
  Schema? schema,
  List<Migration> migrations,
  Uint8List options,
) async {
  final pathBytes = utf8.encode(path);
  final loaded = _io.load(pathBytes, options);
  final Future<_Reply> opened;

  try {
    opened = _call(
      (id, callback) => darudb_open_async(
        loaded,
        pathBytes.length,
        loaded + pathBytes.length,
        options.length,
        id,
        callback,
      ),
    );
  } finally {
    // The library copied the options, which may hold a key or a password,
    // before the call returned.
    _io.wipe(pathBytes.length + options.length);
    options.fillRange(0, options.length, 0);
  }

  final reply = await opened;

  if (reply.status == 0) {
    return Database._(path, reply.pointer<NativeDatabase>(), schema);
  }

  final handle = reply.pointer<NativeTransaction>();
  final (txn, context) = _beginMigration(handle, schema!);

  try {
    while (true) {
      final step = await _call(
        (id, callback) =>
            darudb_migration_next_step_async(handle, id, callback),
      );

      if (step.status == 0) {
        break;
      }

      for (final migration in migrations) {
        if (migration.version == step.count) {
          await migration.run?.call(context);
        }
      }
    }

    final finished = await _call(
      (id, callback) => darudb_migration_finish_async(handle, id, callback),
    );

    return Database._(path, finished.pointer<NativeDatabase>(), schema);
  } finally {
    _endMigration(txn);
  }
}

/// A read transaction of the `Future` API: one commit, as it was when the
/// transaction began.
base class AsyncReadTransaction {
  AsyncReadTransaction._(this._txn);

  final ReadTransaction _txn;
  final _Serial _serial = _Serial();

  /// The collection of [schema], for reading its objects.
  AsyncReadCollection<T, Q, K> collection<
    T,
    Q extends QueryBuilder<T>,
    K extends Object
  >(CollectionSchema<T, Q, K> schema) => AsyncReadCollection<T, Q, K>._(
    this,
    ReadCollection<T, Q, K>._(_txn, schema, _txn._collection(schema)),
  );
}

/// A write transaction of the `Future` API.
final class AsyncWriteTransaction extends AsyncReadTransaction {
  AsyncWriteTransaction._(super._txn) : super._();

  /// The collection of [schema], for reading and writing its objects.
  @override
  AsyncWriteCollection<T, Q, K> collection<
    T,
    Q extends QueryBuilder<T>,
    K extends Object
  >(CollectionSchema<T, Q, K> schema) => AsyncWriteCollection<T, Q, K>._(
    this,
    WriteCollection<T, Q, K>._(
      _txn as WriteTransaction,
      schema,
      _txn._collection(schema),
    ),
  );
}

/// A collection of a transaction of the `Future` API, for reading its
/// objects: what [ReadCollection] offers, each call a `Future`.
base class AsyncReadCollection<T, Q extends QueryBuilder<T>, K extends Object> {
  AsyncReadCollection._(this._owner, this._sync);

  final AsyncReadTransaction _owner;
  final ReadCollection<T, Q, K> _sync;

  /// The collection's name.
  String get name => _sync.name;

  Future<R> _run<R>(Future<R> Function(Pointer<NativeTransaction> txn) call) =>
      _owner._serial.run(() => call(_sync._txn._live()));

  /// The object whose primary key is [key], or `null`.
  Future<T?> get(K key) => _run((txn) async {
    final writer = _io.writer..reset();

    writeKey(writer, key);

    final reply = await _call(
      (id, callback) => darudb_get_async(
        txn,
        _sync._known.name,
        _io.load(writer.written),
        writer.at,
        id,
        callback,
      ),
    );

    return reply.status == 0
        ? null
        : _sync._schema.decode(
            reply.bytes,
            0,
            reply.bytes.length,
            _sync._known.layout,
          );
  });

  Future<List<T>> _find(
    QueryBuilder<T> Function(Q q)? query, {
    required bool first,
  }) => _run((txn) async {
    final writer = _io.writer..reset();

    encodeQuery(writer, _sync._schema.name, _sync._built(query));

    final reply = await _call(
      (id, callback) => darudb_find_async(
        txn,
        _io.load(writer.written),
        writer.at,
        first ? 1 : 0,
        id,
        callback,
      ),
    );

    return _sync._records(reply.bytes);
  });

  /// The objects the query finds; see [ReadCollection.find].
  Future<List<T>> find([QueryBuilder<T> Function(Q q)? query]) =>
      _find(query, first: false);

  /// The first object the query finds, or `null`.
  Future<T?> findOne([QueryBuilder<T> Function(Q q)? query]) async {
    final found = await _find(query, first: true);

    return found.isEmpty ? null : found.first;
  }

  /// How many objects the query finds.
  Future<int> count([QueryBuilder<T> Function(Q q)? query]) =>
      _run((txn) async {
        final writer = _io.writer..reset();

        encodeQuery(writer, _sync._schema.name, _sync._built(query));

        final reply = await _call(
          (id, callback) => darudb_count_async(
            txn,
            _io.load(writer.written),
            writer.at,
            id,
            callback,
          ),
        );

        return reply.count;
      });

  Future<List<T>> _findPrepared(
    Prepared<Object?> prepared,
    List<Object?> parameters, {
    required bool first,
  }) => _run((txn) async {
    final writer = _io.writer..reset();

    encodeParameters(writer, parameters);

    final reply = await _call(
      (id, callback) => darudb_find_prepared_async(
        txn,
        prepared._handle,
        _io.load(writer.written),
        writer.at,
        first ? 1 : 0,
        id,
        callback,
      ),
    );

    return _sync._records(reply.bytes);
  });

  Future<int> _countPrepared(
    Prepared<Object?> prepared,
    List<Object?> parameters,
  ) => _run((txn) async {
    final writer = _io.writer..reset();

    encodeParameters(writer, parameters);

    final reply = await _call(
      (id, callback) => darudb_count_prepared_async(
        txn,
        prepared._handle,
        _io.load(writer.written),
        writer.at,
        id,
        callback,
      ),
    );

    return reply.count;
  });

  /// The objects the query language's [text] finds; see
  /// [ReadCollection.findText].
  Future<List<T>> findText(
    String text, [
    List<Object?> parameters = const [],
  ]) => _findPrepared(_sync._kept(text), parameters, first: false);

  /// The first object [text] finds, or `null`.
  Future<T?> findOneText(
    String text, [
    List<Object?> parameters = const [],
  ]) async {
    final found = await _findPrepared(
      _sync._kept(text),
      parameters,
      first: true,
    );

    return found.isEmpty ? null : found.first;
  }

  /// How many objects [text] finds.
  Future<int> countText(String text, [List<Object?> parameters = const []]) =>
      _countPrepared(_sync._kept(text), parameters);

  /// The objects a prepared query finds.
  Future<List<T>> findPrepared(
    Prepared<T> prepared, [
    List<Object?> parameters = const [],
  ]) => _findPrepared(_sync._preparedOf(prepared), parameters, first: false);

  /// The first object a prepared query finds, or `null`.
  Future<T?> findOnePrepared(
    Prepared<T> prepared, [
    List<Object?> parameters = const [],
  ]) async {
    final found = await _findPrepared(
      _sync._preparedOf(prepared),
      parameters,
      first: true,
    );

    return found.isEmpty ? null : found.first;
  }

  /// How many objects a prepared query finds.
  Future<int> countPrepared(
    Prepared<T> prepared, [
    List<Object?> parameters = const [],
  ]) => _countPrepared(_sync._preparedOf(prepared), parameters);
}

/// A collection of a write transaction of the `Future` API, for reading and
/// writing its objects: what [WriteCollection] offers, each call a
/// `Future`.
final class AsyncWriteCollection<T, Q extends QueryBuilder<T>, K extends Object>
    extends AsyncReadCollection<T, Q, K> {
  AsyncWriteCollection._(super._owner, WriteCollection<T, Q, K> super._sync)
    : super._();

  WriteCollection<T, Q, K> get _writer => _sync as WriteCollection<T, Q, K>;

  Future<List<K>> _write(Iterable<T> objects, {required bool replace}) {
    // The objects are encoded when the call is made, as they are then.
    final writer = Writer(256);

    for (final object in objects) {
      final mark = writer.openLength();

      _sync._schema.encode(object, writer, _sync._known.layout, _writer._sink);
      writer.close(mark);
    }

    return _run((txn) async {
      final reply = await _call(
        (id, callback) => darudb_write_async(
          txn,
          _sync._known.name,
          _io.load(writer.written),
          writer.at,
          replace ? 1 : 0,
          id,
          callback,
        ),
      );
      final reader = Reader(reply.bytes);
      final keys = <K>[];

      while (!reader.isDone) {
        keys.add(_sync._schema.keyOf(reader.any()));
      }

      return keys;
    });
  }

  /// Inserts [object] and returns its primary key; see
  /// [WriteCollection.insert].
  Future<K> insert(T object) async =>
      (await _write([object], replace: false)).single;

  /// Inserts [objects], in one call into the engine, and returns their keys.
  Future<List<K>> insertMany(Iterable<T> objects) =>
      _write(objects, replace: false);

  /// Inserts [object], or replaces the object with its primary key.
  Future<K> put(T object) async =>
      (await _write([object], replace: true)).single;

  /// Inserts or replaces [objects].
  Future<List<K>> putMany(Iterable<T> objects) =>
      _write(objects, replace: true);

  /// Deletes the object whose primary key is [key], and returns whether
  /// there was one.
  Future<bool> delete(K key) {
    final writer = Writer(32);

    writeKey(writer, key);

    return _run((txn) async {
      final reply = await _call(
        (id, callback) => darudb_delete_async(
          txn,
          _sync._known.name,
          _io.load(writer.written),
          writer.at,
          id,
          callback,
        ),
      );

      return reply.status == 1;
    });
  }

  /// Sets the fields [changes] gives in the object whose primary key is
  /// [key]; see [WriteCollection.update].
  Future<bool> update(K key, List<Change> Function(Q q) changes) {
    final keyWriter = Writer(32);
    final changesWriter = Writer(64);

    writeKey(keyWriter, key);
    encodeChanges(
      changesWriter,
      changes(_sync._schema.newQuery()),
      _sync._known.layout,
      _sync._known.names,
    );

    return _run((txn) async {
      final loaded = _io.load(keyWriter.written, changesWriter.written);
      final reply = await _call(
        (id, callback) => darudb_update_async(
          txn,
          _sync._known.name,
          loaded,
          keyWriter.at,
          loaded + keyWriter.at,
          changesWriter.at,
          id,
          callback,
        ),
      );

      return reply.status == 1;
    });
  }
}

/// [Database.readAsync].
Future<R> _readAsync<R>(
  Database database,
  FutureOr<R> Function(AsyncReadTransaction txn) fn,
) async {
  // Beginning a read waits for no writer, so it is made here.
  _check(darudb_begin_read(database._live(), _io.transaction));

  final txn = AsyncReadTransaction._(
    ReadTransaction._(_io.transaction.value, database),
  );

  try {
    return await fn(txn);
  } finally {
    await txn._serial.drain();
    txn._txn._end();
  }
}

/// [Database.writeAsync].
Future<R> _writeAsync<R>(
  Database database,
  FutureOr<R> Function(AsyncWriteTransaction txn) fn,
  Durability durability,
) => _turns.inTurn(database._file, () async {
  final begun = await _call(
    (id, callback) => darudb_begin_write_async(database._live(), id, callback),
  );
  final txn = AsyncWriteTransaction._(
    WriteTransaction._(begun.pointer<NativeTransaction>(), database),
  );

  try {
    final inside = Zone.current[_insideWrite];
    final result = await runZoned(
      () => fn(txn),
      zoneValues: {
        _insideWrite: {if (inside is Set<String>) ...inside, database._file},
      },
    );

    await txn._serial.drain();
    await _call(
      (id, callback) => darudb_commit_async(
        txn._txn._live(),
        durability == Durability.deferred ? 1 : 0,
        id,
        callback,
      ),
    );

    return result;
  } finally {
    await txn._serial.drain();
    txn._txn._end();
  }
});
