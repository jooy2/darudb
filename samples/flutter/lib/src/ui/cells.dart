// How the list draws a field's value in a cell: one line each, cut short
// where it does not fit.
import 'dart:typed_data';

import 'package:flutter/widgets.dart';
import 'package:plass_ui/plass_ui.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/model.dart';
import 'package:darudb_sample/src/ui/format.dart';

// Wrapping stays allowed, though one line is drawn, so that a long value
// does not ask the column for its whole width.
Widget _line(String text, {Color? color}) => Text(
  text,
  maxLines: 1,
  overflow: TextOverflow.ellipsis,
  style: color == null ? null : TextStyle(color: color),
);

Widget cellOf(BuildContext context, FieldInfo field, Object? value) {
  if (value == null) {
    return _line('null', color: PlassTheme.of(context).mutedFg);
  }

  switch (field.kind) {
    case FieldKind.color:
      final Uint8List bytes = value as Uint8List;
      final String hex = bytes
          .map((int byte) => byte.toRadixString(16).padLeft(2, '0'))
          .join();

      return Align(
        alignment: AlignmentDirectional.centerStart,
        child: PlTooltip(
          content: Text('#$hex'),
          child: Container(
            width: 16,
            height: 16,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: Color.fromARGB(255, bytes[0], bytes[1], bytes[2]),
            ),
          ),
        ),
      );
    case FieldKind.boolean:
      return _line(value as bool ? 'yes' : 'no');
    case FieldKind.date:
      return _line(formatDate(value as int));
    case FieldKind.float:
      return _line((value as double).toStringAsFixed(1));
    case FieldKind.tags:
      return _line((value as List<String>).join(', '));
    case FieldKind.location:
      final Place place = value as Place;

      return _line(
        <String?>[place.city, place.region, place.country].nonNulls.join(', '),
      );
    default:
      return _line(value.toString());
  }
}
