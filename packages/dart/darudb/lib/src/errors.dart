/// The error every failure of the package throws, with the engine's stable
/// code.
library;

/// A failure of the database, with the engine's error [code], the same
/// string in every language DaruDB ships to: `BUSY`, `DUPLICATE_KEY`,
/// `INVALID_QUERY`, `CORRUPTED` and the rest.
final class DaruException implements Exception {
  const DaruException(this.code, this.message);

  /// The stable, machine-readable name of the failure.
  final String code;

  /// What went wrong, for a person.
  final String message;

  @override
  String toString() => 'DaruException($code): $message';
}
