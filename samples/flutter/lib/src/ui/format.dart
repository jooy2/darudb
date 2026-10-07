// How the screens write numbers, sizes, dates and durations.

/// A count with a comma every three digits, `12,345`.
String formatCount(int value) {
  final String digits = value.abs().toString();
  final StringBuffer buffer = StringBuffer(value < 0 ? '-' : '');

  for (int i = 0; i < digits.length; i += 1) {
    if (i > 0 && (digits.length - i) % 3 == 0) {
      buffer.write(',');
    }

    buffer.write(digits[i]);
  }

  return buffer.toString();
}

String formatBytes(int bytes) {
  const List<String> units = <String>['B', 'KiB', 'MiB', 'GiB'];
  double value = bytes.toDouble();
  int unit = 0;

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return unit == 0 ? '$bytes B' : '${value.toStringAsFixed(1)} ${units[unit]}';
}

/// A time in milliseconds since 1970 as its date, `2024-05-17`.
String formatDate(int ms) => DateTime.fromMillisecondsSinceEpoch(
  ms,
  isUtc: true,
).toIso8601String().substring(0, 10);

/// A duration in milliseconds: `840 ms`, or `3.2 s` past a second.
String formatDuration(int ms) =>
    ms < 1000 ? '$ms ms' : '${(ms / 1000).toStringAsFixed(1)} s';

/// Objects per second, from a count and the milliseconds it took.
String formatRate(int count, int ms) =>
    ms <= 0 ? '' : '${formatCount((count / ms * 1000).round())} objects/s';
