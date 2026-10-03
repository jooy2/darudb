/// The engine's C interface, as `native/src/lib.rs` defines it: the
/// package's internals, not its API. The build hook builds the library and
/// gives it this library's asset id, which every function here names.
///
/// A function that can fail returns `-1` and leaves its error for
/// [darudb_last_error]; bytes it hands out stay valid until the next call
/// on the thread, which a synchronous caller reads before anything else.
// The names are the C functions' own, which `@Native` finds them by.
// ignore_for_file: non_constant_identifier_names
@DefaultAsset('package:darudb/src/native.dart')
library;

import 'dart:ffi';

/// Bytes the library hands out.
final class Buf extends Struct {
  external Pointer<Uint8> ptr;

  @Size()
  external int len;
}

/// What a database tells about itself.
final class Info extends Struct {
  @Uint32()
  external int pageSize;

  @Uint32()
  external int formatVersion;

  @Uint8()
  external int encrypted;
}

/// An open database.
final class NativeDatabase extends Opaque {}

/// A transaction, or a migration under way.
final class NativeTransaction extends Opaque {}

/// A prepared query.
final class NativePrepared extends Opaque {}

/// A collection's name, made once.
final class NativeName extends Opaque {}

@Native<Void Function(Pointer<Buf>, Pointer<Buf>)>()
external void darudb_last_error(Pointer<Buf> code, Pointer<Buf> message);

@Native<Void Function(Pointer<Buf>)>()
external void darudb_engine_version(Pointer<Buf> out);

@Native<Uint32 Function()>()
external int darudb_format_version();

@Native<
  Int32 Function(
    Pointer<Uint8>,
    Size,
    Pointer<Uint8>,
    Size,
    Pointer<Pointer<NativeDatabase>>,
    Pointer<Pointer<NativeTransaction>>,
  )
>()
external int darudb_open(
  Pointer<Uint8> path,
  int pathLength,
  Pointer<Uint8> options,
  int optionsLength,
  Pointer<Pointer<NativeDatabase>> database,
  Pointer<Pointer<NativeTransaction>> migration,
);

@Native<Void Function(Pointer<NativeDatabase>)>()
external void darudb_database_free(Pointer<NativeDatabase> database);

@Native<Int32 Function(Pointer<NativeDatabase>)>()
external int darudb_close(Pointer<NativeDatabase> database);

@Native<Int32 Function(Pointer<NativeDatabase>, Pointer<Info>)>()
external int darudb_database_info(
  Pointer<NativeDatabase> database,
  Pointer<Info> info,
);

@Native<Int32 Function(Pointer<NativeDatabase>, Pointer<Buf>)>()
external int darudb_schema_record(
  Pointer<NativeDatabase> database,
  Pointer<Buf> out,
);

@Native<
  Int32 Function(Pointer<NativeDatabase>, Pointer<Pointer<NativeTransaction>>)
>()
external int darudb_begin_read(
  Pointer<NativeDatabase> database,
  Pointer<Pointer<NativeTransaction>> txn,
);

@Native<
  Int32 Function(Pointer<NativeDatabase>, Pointer<Pointer<NativeTransaction>>)
>()
external int darudb_begin_write(
  Pointer<NativeDatabase> database,
  Pointer<Pointer<NativeTransaction>> txn,
);

@Native<Int32 Function(Pointer<NativeDatabase>)>()
external int darudb_sync(Pointer<NativeDatabase> database);

@Native<Int32 Function(Pointer<NativeDatabase>, Pointer<Uint8>, Size)>()
external int darudb_set_key(
  Pointer<NativeDatabase> database,
  Pointer<Uint8> key,
  int keyLength,
);

@Native<Int32 Function(Pointer<NativeDatabase>, Pointer<Uint8>, Size)>()
external int darudb_set_password(
  Pointer<NativeDatabase> database,
  Pointer<Uint8> password,
  int passwordLength,
);

@Native<Void Function(Pointer<NativeTransaction>)>()
external void darudb_txn_free(Pointer<NativeTransaction> txn);

@Native<Void Function(Pointer<NativeTransaction>)>()
external void darudb_txn_end(Pointer<NativeTransaction> txn);

@Native<Int32 Function(Pointer<NativeTransaction>, Uint8)>()
external int darudb_commit(Pointer<NativeTransaction> txn, int deferred);

@Native<Pointer<NativeName> Function(Pointer<Uint8>, Size)>()
external Pointer<NativeName> darudb_name(Pointer<Uint8> name, int nameLength);

@Native<Void Function(Pointer<NativeName>)>()
external void darudb_name_free(Pointer<NativeName> name);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<NativeName>,
    Pointer<Uint8>,
    Size,
    Pointer<Buf>,
  )
>()
external int darudb_get(
  Pointer<NativeTransaction> txn,
  Pointer<NativeName> name,
  Pointer<Uint8> key,
  int keyLength,
  Pointer<Buf> out,
);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<Uint8>,
    Size,
    Uint8,
    Pointer<Buf>,
  )
>()
external int darudb_find(
  Pointer<NativeTransaction> txn,
  Pointer<Uint8> ir,
  int irLength,
  int first,
  Pointer<Buf> out,
);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<Uint8>,
    Size,
    Pointer<Uint64>,
  )
>()
external int darudb_count(
  Pointer<NativeTransaction> txn,
  Pointer<Uint8> ir,
  int irLength,
  Pointer<Uint64> count,
);

@Native<
  Int32 Function(
    Pointer<Uint8>,
    Size,
    Pointer<Uint8>,
    Size,
    Pointer<Pointer<NativePrepared>>,
  )
>()
external int darudb_prepare_text(
  Pointer<Uint8> name,
  int nameLength,
  Pointer<Uint8> query,
  int queryLength,
  Pointer<Pointer<NativePrepared>> prepared,
);

@Native<
  Int32 Function(Pointer<Uint8>, Size, Pointer<Pointer<NativePrepared>>)
>()
external int darudb_prepare_ir(
  Pointer<Uint8> ir,
  int irLength,
  Pointer<Pointer<NativePrepared>> prepared,
);

@Native<Void Function(Pointer<NativePrepared>)>()
external void darudb_prepared_free(Pointer<NativePrepared> prepared);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<NativePrepared>,
    Pointer<Uint8>,
    Size,
    Uint8,
    Pointer<Buf>,
  )
>()
external int darudb_find_prepared(
  Pointer<NativeTransaction> txn,
  Pointer<NativePrepared> prepared,
  Pointer<Uint8> parameters,
  int parametersLength,
  int first,
  Pointer<Buf> out,
);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<NativePrepared>,
    Pointer<Uint8>,
    Size,
    Pointer<Uint64>,
  )
>()
external int darudb_count_prepared(
  Pointer<NativeTransaction> txn,
  Pointer<NativePrepared> prepared,
  Pointer<Uint8> parameters,
  int parametersLength,
  Pointer<Uint64> count,
);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<NativeName>,
    Pointer<Uint8>,
    Size,
    Uint8,
    Pointer<Buf>,
  )
>()
external int darudb_write(
  Pointer<NativeTransaction> txn,
  Pointer<NativeName> name,
  Pointer<Uint8> records,
  int recordsLength,
  int replace,
  Pointer<Buf> out,
);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<NativeName>,
    Pointer<Uint8>,
    Size,
    Pointer<Uint8>,
    Size,
  )
>()
external int darudb_update(
  Pointer<NativeTransaction> txn,
  Pointer<NativeName> name,
  Pointer<Uint8> key,
  int keyLength,
  Pointer<Uint8> changes,
  int changesLength,
);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<NativeName>,
    Pointer<Uint8>,
    Size,
  )
>()
external int darudb_delete(
  Pointer<NativeTransaction> txn,
  Pointer<NativeName> name,
  Pointer<Uint8> key,
  int keyLength,
);

@Native<Int32 Function(Pointer<NativeTransaction>, Pointer<Uint64>)>()
external int darudb_migration_previous_version(
  Pointer<NativeTransaction> txn,
  Pointer<Uint64> version,
);

@Native<Int32 Function(Pointer<NativeTransaction>, Uint8, Pointer<Buf>)>()
external int darudb_migration_schema_record(
  Pointer<NativeTransaction> txn,
  int previous,
  Pointer<Buf> out,
);

@Native<Int32 Function(Pointer<NativeTransaction>, Pointer<Uint64>)>()
external int darudb_migration_next_step(
  Pointer<NativeTransaction> txn,
  Pointer<Uint64> version,
);

@Native<
  Int32 Function(
    Pointer<NativeTransaction>,
    Pointer<NativeName>,
    Pointer<Uint8>,
    Size,
    Pointer<Buf>,
  )
>()
external int darudb_migration_previous_record(
  Pointer<NativeTransaction> txn,
  Pointer<NativeName> name,
  Pointer<Uint8> key,
  int keyLength,
  Pointer<Buf> out,
);

@Native<
  Int32 Function(Pointer<NativeTransaction>, Pointer<NativeName>, Pointer<Buf>)
>()
external int darudb_migration_previous_keys(
  Pointer<NativeTransaction> txn,
  Pointer<NativeName> name,
  Pointer<Buf> out,
);

@Native<
  Int32 Function(Pointer<NativeTransaction>, Pointer<Pointer<NativeDatabase>>)
>()
external int darudb_migration_finish(
  Pointer<NativeTransaction> txn,
  Pointer<Pointer<NativeDatabase>> database,
);
