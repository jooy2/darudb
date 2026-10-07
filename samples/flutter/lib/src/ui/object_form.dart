// The dialog that adds an object or changes one. Each field gets the input
// its kind needs, and saving writes the whole object: an insert for a new
// one, a `put` over the old one otherwise. What the engine refuses, such as a
// nickname another person holds, shows in the dialog with the engine's code,
// and the dialog stays open.
//
// Only what a form cannot leave to the engine is checked here, such as a
// required number that is empty.
import 'dart:typed_data';

import 'package:darudb/darudb.dart';
import 'package:flutter/widgets.dart';
import 'package:plass_ui/plass_ui.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/model.dart';
import 'package:darudb_sample/src/store.dart';
import 'package:darudb_sample/src/ui/format.dart';

const Map<String, String> _descriptions = <String, String>{
  'tags': 'Separate tags with commas.',
  'organization': 'The code of an organization, such as ORG-000001.',
  'author': 'The id of a person.',
  'joinedAt': 'A date, such as 2024-05-17.',
  'createdAt': 'A date, such as 2024-05-17.',
};

/// The key a test finds a field's input by.
Key fieldKey(String name) => Key('field-$name');

class ObjectForm extends StatefulWidget {
  const ObjectForm({
    super.key,
    required this.store,
    required this.collection,
    required this.row,
    required this.onClose,
    required this.onSaved,
  });

  final SampleStore store;
  final SampleCollection collection;

  /// The object to change, or `null` for a new one.
  final SampleRow? row;
  final VoidCallback onClose;
  final void Function(Object key, {required bool inserted}) onSaved;

  @override
  State<ObjectForm> createState() => _ObjectFormState();
}

class _ObjectFormState extends State<ObjectForm> {
  final Map<String, TextEditingController> _texts =
      <String, TextEditingController>{};
  final Map<String, double?> _numbers = <String, double?>{};
  final Map<String, bool> _switches = <String, bool>{};
  final Map<String, String> _colors = <String, String>{};
  final Map<String, String> _errors = <String, String>{};
  DaruException? _failure;
  bool _saving = false;

  CollectionInfo get _info => widget.collection.info;

  Object? get _key => widget.row?[_info.key];

  @override
  void initState() {
    super.initState();

    for (final FieldInfo field in _info.fields) {
      _fill(field, widget.row?[field.name]);
    }
  }

  @override
  void dispose() {
    for (final TextEditingController controller in _texts.values) {
      controller.dispose();
    }

    super.dispose();
  }

  TextEditingController _text(String name, [String text = '']) =>
      _texts.putIfAbsent(name, () => TextEditingController(text: text));

  void _fill(FieldInfo field, Object? value) {
    switch (field.kind) {
      case FieldKind.integer:
        _numbers[field.name] =
            (value as int?)?.toDouble() ?? (field.optional ? null : 0);
      case FieldKind.float:
        _numbers[field.name] = (value as double?) ?? 0;
      case FieldKind.boolean:
        _switches[field.name] = (value as bool?) ?? field.name == 'active';
      case FieldKind.color:
        final Uint8List? bytes = value as Uint8List?;

        _colors[field.name] = bytes == null
            ? '#3d7be0'
            : '#${bytes.map((int byte) => byte.toRadixString(16).padLeft(2, '0')).join()}';
      case FieldKind.date:
        _text(
          field.name,
          formatDate((value as int?) ?? DateTime.now().millisecondsSinceEpoch),
        );
      case FieldKind.tags:
        _text(field.name, (value as List<String>?)?.join(', ') ?? '');
      case FieldKind.location:
        final Place? place = value as Place?;

        _text('${field.name}.country', place?.country ?? '');
        _text('${field.name}.region', place?.region ?? '');
        _text('${field.name}.city', place?.city ?? '');
      case FieldKind.string:
      case FieldKind.link:
        _text(field.name, value?.toString() ?? '');
    }
  }

  String? _optionalText(String name) {
    final String text = _text(name).text.trim();

    return text.isEmpty ? null : text;
  }

  Object? _valueOf(FieldInfo field) {
    switch (field.kind) {
      case FieldKind.integer:
        final double? number = _numbers[field.name];

        if (number == null) {
          if (field.optional) {
            return null;
          }

          throw const FormatException('needs a number');
        }

        if (number != number.roundToDouble()) {
          throw const FormatException('needs a whole number');
        }

        return number.round();
      case FieldKind.float:
        return _numbers[field.name] ?? 0.0;
      case FieldKind.boolean:
        return _switches[field.name];
      case FieldKind.color:
        final String hex = (_colors[field.name] ?? '').replaceFirst('#', '');

        return Uint8List.fromList(<int>[
          for (int i = 0; i + 1 < hex.length && i < 6; i += 2)
            int.parse(hex.substring(i, i + 2), radix: 16),
        ]);
      case FieldKind.date:
        final DateTime? date = DateTime.tryParse(
          '${_text(field.name).text.trim()}T00:00:00Z',
        );

        if (date == null) {
          throw const FormatException('needs a date');
        }

        return date.millisecondsSinceEpoch;
      case FieldKind.tags:
        final List<String> tags = <String>[
          for (final String tag in _text(field.name).text.split(','))
            if (tag.trim().isNotEmpty) tag.trim(),
        ];

        return tags.isEmpty && field.optional ? null : tags;
      case FieldKind.location:
        final String? country = _optionalText('${field.name}.country');

        if (country == null) {
          if (field.optional) {
            return null;
          }

          throw const FormatException('needs a country');
        }

        return Place(
          country: country,
          region: _optionalText('${field.name}.region'),
          city: _optionalText('${field.name}.city'),
        );
      case FieldKind.link:
        final String? text = _optionalText(field.name);

        if (text == null) {
          if (field.optional) {
            return null;
          }

          throw const FormatException('needs a key');
        }

        if (field.target != SampleCollection.people) {
          return text;
        }

        final int? key = int.tryParse(text);

        if (key == null || key < 1) {
          throw const FormatException('needs the id of a person');
        }

        return key;
      case FieldKind.string:
        final String text = _text(field.name).text.trim();

        return text.isEmpty && field.optional ? null : text;
    }
  }

  Future<void> _handleSavePressed() async {
    final SampleRow row = <String, Object?>{};
    final Map<String, String> errors = <String, String>{};

    for (final FieldInfo field in _info.fields) {
      try {
        row[field.name] = _valueOf(field);
      } on FormatException catch (error) {
        errors[field.name] = '${field.name} ${error.message}';
      }
    }

    setState(() {
      _errors
        ..clear()
        ..addAll(errors);
      _failure = null;
    });

    if (errors.isNotEmpty) {
      return;
    }

    setState(() => _saving = true);

    try {
      final Object? key = _key;

      if (key == null) {
        final Object inserted = await widget.store.insert(
          widget.collection,
          row,
        );

        widget.onSaved(inserted, inserted: true);
      } else {
        await widget.store.replace(widget.collection, key, row);
        widget.onSaved(key, inserted: false);
      }
    } on DaruException catch (error) {
      if (mounted) {
        setState(() => _failure = error);
      }
    } finally {
      if (mounted) {
        setState(() => _saving = false);
      }
    }
  }

  Widget? _errorOf(String name) {
    final String? error = _errors[name];

    return error == null ? null : Text(error);
  }

  Widget _getFieldWidget(FieldInfo field) {
    final String label = field.optional
        ? '${field.name} (optional)'
        : field.name;
    final String? description = _descriptions[field.name];

    switch (field.kind) {
      case FieldKind.integer:
      case FieldKind.float:
        return PlNumberField(
          key: fieldKey(field.name),
          label: Text(label),
          value: _numbers[field.name],
          onChanged: (double? value) =>
              setState(() => _numbers[field.name] = value),
          step: field.kind == FieldKind.integer ? 1 : 0.1,
          error: _errorOf(field.name),
          fullWidth: true,
        );
      case FieldKind.boolean:
        return PlSwitch(
          key: fieldKey(field.name),
          label: Text(field.name),
          value: _switches[field.name] ?? false,
          onChanged: (bool value) =>
              setState(() => _switches[field.name] = value),
        );
      case FieldKind.color:
        return PlColorPicker(
          key: fieldKey(field.name),
          label: Text(field.name),
          value: _colors[field.name],
          onValueChanged: (String value) =>
              setState(() => _colors[field.name] = value),
        );
      case FieldKind.location:
        return PlFieldset(
          legend: Text(label),
          description: _errorOf(field.name),
          children: <Widget>[
            for (final String part in <String>['country', 'region', 'city'])
              PlTextField(
                key: fieldKey('${field.name}.$part'),
                label: Text(part == 'country' ? part : '$part (optional)'),
                controller: _texts['${field.name}.$part'],
                fullWidth: true,
              ),
          ],
        );
      case FieldKind.string:
      case FieldKind.date:
      case FieldKind.link:
      case FieldKind.tags:
        return PlTextField(
          key: fieldKey(field.name),
          label: Text(label),
          controller: _texts[field.name],
          description: description == null ? null : Text(description),
          error: _errorOf(field.name),
          readOnly: _key != null && field.name == _info.key,
          multiline: field.name == 'body',
          fullWidth: true,
        );
    }
  }

  @override
  Widget build(BuildContext context) {
    final Object? key = _key;
    final DaruException? failure = _failure;
    final String title = key == null
        ? 'New object in ${widget.collection.name}'
        : 'Edit ${widget.collection.name} $key';

    return PlModal(
      open: true,
      onOpenChanged: (bool open) {
        if (!open) {
          widget.onClose();
        }
      },
      title: Text(title),
      size: PlassSize.lg,
      actions: <Widget>[
        PlButton(
          variant: PlassVariant.ghost,
          onPressed: widget.onClose,
          child: const Text('Cancel'),
        ),
        PlButton(
          key: const Key('save'),
          loading: _saving,
          onPressed: _handleSavePressed,
          child: const Text('Save'),
        ),
      ],
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: 12,
        children: <Widget>[
          if (failure != null)
            PlAlert(
              key: const Key('form-failure'),
              color: PlassColor.danger,
              title: Text(failure.code),
              child: Text(failure.message),
            ),
          for (final FieldInfo field in _info.fields) _getFieldWidget(field),
        ],
      ),
    );
  }
}
