part of 'database.dart';

// The tools: the integrity check, backup, compaction and salvage, which the
// engine runs and reports on, here and through the `Future` API.

/// What the integrity check found: the commit it checked, how much it read,
/// and every problem.
final class CheckReport {
  CheckReport._(Map<int, Object?> record)
    : commitId = record[1]! as int,
      pageCount = record[2]! as int,
      pagesChecked = record[3]! as int,
      objectsChecked = record[4]! as int,
      problems = [
        for (final problem in (record[5] as List?) ?? const [])
          CheckProblem._(problem as Map<int, Object?>),
      ];

  /// The transaction id of the commit checked.
  final int commitId;

  /// The pages of the file.
  final int pageCount;

  /// The pages read and checked.
  final int pagesChecked;

  /// The objects checked against their indexes.
  final int objectsChecked;

  /// Everything wrong that was found; empty when the file is sound.
  final List<CheckProblem> problems;

  /// Whether no problem was found.
  bool get ok => problems.isEmpty;
}

/// A problem the integrity check found.
final class CheckProblem {
  CheckProblem._(Map<int, Object?> record)
    : page = record[1] as int?,
      tree = record[2] as String?,
      message = record[3]! as String;

  /// The page it is on, when known.
  final int? page;

  /// The tree it is in, when known.
  final String? tree;

  /// What is wrong.
  final String message;

  @override
  String toString() => [
    if (page != null) 'page $page',
    if (tree != null) 'tree $tree',
    message,
  ].join(': ');
}

/// What a backup wrote.
final class BackupReport {
  BackupReport._(Map<int, Object?> record)
    : commitId = record[1]! as int,
      trees = record[2]! as int,
      entries = record[3]! as int,
      bytes = record[4]! as int;

  /// The transaction id of the commit copied.
  final int commitId;

  /// The trees of the copy, the engine's own included.
  final int trees;

  /// The entries of the copy.
  final int entries;

  /// The size of the copy, in bytes.
  final int bytes;
}

/// What compaction did.
final class CompactReport {
  CompactReport._(Map<int, Object?> record)
    : bytesBefore = record[1]! as int,
      bytesAfter = record[2]! as int,
      pagesMoved = record[3]! as int;

  /// The size of the file before, in bytes.
  final int bytesBefore;

  /// The size of the file after, in bytes.
  final int bytesAfter;

  /// The pages moved toward the start of the file.
  final int pagesMoved;
}

/// What salvage rescued.
final class SalvageReport {
  SalvageReport._(Map<int, Object?> record, this.whole)
    : commitId = record[1] as int?,
      pagesScanned = record[2]! as int,
      pagesDamaged = record[3]! as int,
      pagesUnread = record[4]! as int,
      entriesRecovered = record[5]! as int,
      valuesLost = record[6]! as int,
      objectsDropped = record[7]! as int,
      trees = record[8]! as int,
      entries = record[9]! as int,
      bytes = record[10]! as int;

  /// Whether the new file holds exactly the commit salvage started from:
  /// every page of it was read, and no object was dropped.
  final bool whole;

  /// The transaction id of the commit salvage started from, or `null` when
  /// no commit record could be used and every tree came from the pages
  /// found.
  final int? commitId;

  /// The pages of the file read, the header page left out.
  final int pagesScanned;

  /// The pages that failed their check, other than pages never written.
  final int pagesDamaged;

  /// The pages of the commit that could not be read, whose contents were
  /// taken from older versions of the same pages where the file had them.
  final int pagesUnread;

  /// The entries taken from those older versions.
  final int entriesRecovered;

  /// The keys left out because no version of their value could be read.
  final int valuesLost;

  /// The objects left out: unreadable ones, ones whose unique value another
  /// object had taken, and every object of a file whose schema was lost.
  final int objectsDropped;

  /// The trees of the new file, the engine's own included.
  final int trees;

  /// The entries of the new file, the indexes' included.
  final int entries;

  /// The size of the new file, in bytes.
  final int bytes;
}

Map<int, Object?> _report(Uint8List bytes) => Reader(bytes).anyFields();

/// The options salvage reads, as `Database.open`'s record holds them.
/// The options of `Database.backup` as the record the library reads: the
/// key, the password and its hashing cost of the copy.
Uint8List _backupOptions(
  Uint8List? key,
  String? password,
  PasswordHashing? passwordHashing,
) {
  if (key != null && password != null) {
    throw invalidArgument('give a key or a password, not both');
  }

  return _options(
    create: false,
    pageSize: null,
    busyTimeout: null,
    cacheSize: null,
    schema: null,
    key: key,
    password: password,
    passwordHashing: passwordHashing,
    migrations: const [],
  );
}

Uint8List _salvageOptions(
  Duration? busyTimeout,
  Uint8List? key,
  String? password,
) => _options(
  create: false,
  pageSize: null,
  busyTimeout: busyTimeout,
  cacheSize: null,
  schema: null,
  key: key,
  password: password,
  passwordHashing: null,
  migrations: const [],
);
