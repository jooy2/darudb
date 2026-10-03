/// Writes, for each class annotated `@Collection()` or `@Embedded()` in a
/// library, what the `darudb` package needs to store it: its schema constant,
/// the functions that write it as a record and read it from one, its query
/// fields, and a `copyWith`.
///
/// The annotations are found by name in the `darudb` package rather than by
/// type, so the generator does not depend on the runtime package and the
/// native library it builds.
library;

import 'package:analyzer/dart/constant/value.dart';
import 'package:analyzer/dart/element/element.dart';
import 'package:analyzer/dart/element/nullability_suffix.dart';
import 'package:analyzer/dart/element/type.dart';
import 'package:build/build.dart';
import 'package:source_gen/source_gen.dart';

const _collection = TypeChecker.typeNamedLiterally(
  'Collection',
  inPackage: 'darudb',
);
const _embedded = TypeChecker.typeNamedLiterally(
  'Embedded',
  inPackage: 'darudb',
);
const _primaryKey = TypeChecker.typeNamedLiterally(
  'PrimaryKey',
  inPackage: 'darudb',
);
const _index = TypeChecker.typeNamedLiterally('Index', inPackage: 'darudb');
const _unique = TypeChecker.typeNamedLiterally('Unique', inPackage: 'darudb');
const _name = TypeChecker.typeNamedLiterally('Name', inPackage: 'darudb');
const _link = TypeChecker.typeNamedLiterally('Link', inPackage: 'darudb');

/// The generator `build_runner` runs on each library.
final class DaruGenerator extends Generator {
  const DaruGenerator();

  @override
  String? generate(LibraryReader library, BuildStep buildStep) {
    final output = StringBuffer();

    for (final annotated in library.annotatedWith(_embedded)) {
      output.writeln(_Embedded(_classOf(annotated.element, 'Embedded')).code());
    }

    for (final annotated in library.annotatedWith(_collection)) {
      final element = _classOf(annotated.element, 'Collection');
      final name = annotated.annotation.peek('name')?.stringValue;

      output.writeln(_Collection(element, name ?? element.name!).code());
    }

    return output.isEmpty ? null : output.toString();
  }
}

ClassElement _classOf(Element element, String annotation) {
  if (element is! ClassElement) {
    throw InvalidGenerationSourceError(
      '`@$annotation()` goes on a class.',
      element: element,
    );
  }

  if (element.typeParameters.isNotEmpty) {
    throw InvalidGenerationSourceError(
      '`${element.name}` cannot have type parameters.',
      element: element,
    );
  }

  return element;
}

String _lowerFirst(String name) =>
    name.isEmpty ? name : name[0].toLowerCase() + name.substring(1);

/// The name of the schema constant of class [name]: `userSchema` for
/// `User`.
String schemaName(String name) => '${_lowerFirst(name)}Schema';

/// What a field's Dart type means for storing it.
final class _Type {
  const _Type({
    required this.kind,
    required this.write,
    required this.read,
    required this.readNullable,
    required this.field,
    this.fieldIsGenerated = false,
  });

  /// The `Kind` expression.
  final String kind;

  /// The `FieldSink` method, given the value.
  final String Function(String value) write;

  /// The `FieldSource` call for a required field.
  final String read;

  /// The `FieldSource` call for an optional field.
  final String readNullable;

  /// The query field class.
  final String field;

  /// Whether [field] is a class this generator writes, which takes only a
  /// path.
  final bool fieldIsGenerated;
}

/// A field of an annotated class.
final class _Field {
  _Field(this.element, this.slotName, this.type, this.dartType);

  final FieldElement element;

  /// The field's name in the collection.
  final String slotName;
  final _Type type;
  final DartType dartType;

  bool primaryKey = false;
  bool index = false;
  bool unique = false;
  String? defaultCode;

  String get name => element.name!;

  bool get optional => dartType.nullabilitySuffix == NullabilitySuffix.question;
}

/// The fields of [element], in declaration order, with their types read.
List<_Field> _fieldsOf(ClassElement element, {required bool embedded}) {
  final constructor = element.unnamedConstructor;

  if (constructor == null) {
    throw InvalidGenerationSourceError(
      '`${element.name}` needs an unnamed constructor that takes every field.',
      element: element,
    );
  }

  final parameters = {
    for (final parameter in constructor.formalParameters)
      parameter.name: parameter,
  };
  final fields = <_Field>[];

  for (final field in element.fields) {
    if (field.isStatic || !field.isOriginDeclaration) {
      continue;
    }

    if (!field.isFinal) {
      throw InvalidGenerationSourceError(
        '`${element.name}.${field.name}` is not final; objects read from the '
        'database are values, so every field is final.',
        element: field,
      );
    }

    final parameter = parameters[field.name];

    if (parameter == null) {
      throw InvalidGenerationSourceError(
        "`${element.name}`'s unnamed constructor does not take "
        '`${field.name}`.',
        element: field,
      );
    }

    final renamed = _name.firstAnnotationOfExact(field)?.getField('name');
    final parsed =
        _Field(
            field,
            renamed?.toStringValue() ?? field.name!,
            _typeOf(field.type, field),
            field.type,
          )
          ..primaryKey = _primaryKey.hasAnnotationOfExact(field)
          ..index = _index.hasAnnotationOfExact(field)
          ..unique = _unique.hasAnnotationOfExact(field);

    if (embedded && (parsed.primaryKey || parsed.index || parsed.unique)) {
      throw InvalidGenerationSourceError(
        '`${element.name}.${field.name}` is in an embedded object, which has '
        'no key, and no index reaches inside one.',
        element: field,
      );
    }

    if (parameter.hasDefaultValue) {
      final value = parameter.computeConstantValue();

      if (parsed.optional) {
        throw InvalidGenerationSourceError(
          '`${element.name}.${field.name}` is nullable and has a default; a '
          'field with a default is required, so drop the `?` or the default.',
          element: field,
        );
      }

      if (value == null || !_storable(value)) {
        throw InvalidGenerationSourceError(
          'The default of `${element.name}.${field.name}` is not a constant '
          'the database can store: a bool, int, double, String or a list of '
          'them.',
          element: parameter,
        );
      }

      // The schema constant is const already, so a `const` inside it is
      // one the analyzer calls unnecessary.
      parsed.defaultCode = parameter.defaultValueCode?.replaceFirst(
        RegExp(r'^const\s+'),
        '',
      );
    }

    if (fields.any((other) => other.slotName == parsed.slotName)) {
      throw InvalidGenerationSourceError(
        'Two fields of `${element.name}` are called `${parsed.slotName}`.',
        element: field,
      );
    }

    fields.add(parsed);
  }

  return fields;
}

bool _storable(DartObject value) {
  final type = value.type;

  if (type == null) {
    return false;
  }

  if (type.isDartCoreBool ||
      type.isDartCoreInt ||
      type.isDartCoreDouble ||
      type.isDartCoreString) {
    return true;
  }

  return type.isDartCoreList &&
      (value.toListValue()?.every(_storable) ?? false);
}

bool _isUint8List(DartType type) =>
    type is InterfaceType &&
    type.element.name == 'Uint8List' &&
    type.element.library.uri.toString() == 'dart:typed_data';

/// The meaning of [type], a field's type, or of a list's element.
_Type _typeOf(DartType type, Element where, {bool element = false}) {
  final base = type.getDisplayString().replaceAll(RegExp(r'\?$'), '');

  if (type.isDartCoreBool) {
    return _Type(
      kind: 'BoolKind()',
      write: (value) => 'sink.boolean($value)',
      read: 'source.boolean()',
      readNullable: 'source.booleanOrNull()',
      field: 'BoolField',
    );
  }

  if (type.isDartCoreInt) {
    return _Type(
      kind: 'IntKind()',
      write: (value) => 'sink.int64($value)',
      read: 'source.int64()',
      readNullable: 'source.int64OrNull()',
      field: 'IntField',
    );
  }

  if (type.isDartCoreDouble) {
    return _Type(
      kind: 'FloatKind()',
      write: (value) => 'sink.float($value)',
      read: 'source.float()',
      readNullable: 'source.floatOrNull()',
      field: 'FloatField',
    );
  }

  if (type.isDartCoreString) {
    return _Type(
      kind: 'StringKind()',
      write: (value) => 'sink.string($value)',
      read: 'source.string()',
      readNullable: 'source.stringOrNull()',
      field: 'StringField',
    );
  }

  if (_isUint8List(type)) {
    return _Type(
      kind: 'BytesKind()',
      write: (value) => 'sink.bytes($value)',
      read: 'source.bytes()',
      readNullable: 'source.bytesOrNull()',
      field: 'BytesField',
    );
  }

  if (type is InterfaceType && _link.isExactlyType(type)) {
    final target = type.typeArguments.single.element;

    if (target is! ClassElement || !_collection.hasAnnotationOfExact(target)) {
      throw InvalidGenerationSourceError(
        'A `Link` points to a class annotated `@Collection()`.',
        element: where,
      );
    }

    final collection =
        _collection
            .firstAnnotationOfExact(target)
            ?.getField('name')
            ?.toStringValue() ??
        target.name!;

    return _Type(
      kind: "LinkKind('$collection')",
      write: (value) => 'sink.link($value)',
      read: 'source.link<${target.name}>()',
      readNullable: 'source.linkOrNull<${target.name}>()',
      field: '${target.name}Link',
      fieldIsGenerated: true,
    );
  }

  if (!element && type is InterfaceType && type.isDartCoreList) {
    final elementType = type.typeArguments.single;

    if (elementType.nullabilitySuffix == NullabilitySuffix.question) {
      throw InvalidGenerationSourceError(
        'A list holds no null: `$base` has a nullable element.',
        element: where,
      );
    }

    final inner = _typeOf(elementType, where, element: true);
    final elementName = elementType.getDisplayString();
    final field = switch (inner.field) {
      'StringField' => 'StringListField',
      _ when inner.fieldIsGenerated => 'ListField<Object>',
      _ => 'ListField<$elementName>',
    };
    // A link's type argument is part of its runtime type, so a list of them
    // is read by a method that makes each with it.
    final links =
        elementType is InterfaceType && _link.isExactlyType(elementType);
    final target = links
        ? elementType.typeArguments.single.getDisplayString()
        : '';

    return _Type(
      kind: 'ListKind(${inner.kind})',
      write: (value) => 'sink.list($value)',
      read: links
          ? 'source.linkList<$target>()'
          : 'source.list<$elementName>()',
      readNullable: links
          ? 'source.linkListOrNull<$target>()'
          : 'source.listOrNull<$elementName>()',
      field: field,
    );
  }

  final target = type.element;

  if (!element &&
      target is ClassElement &&
      _embedded.hasAnnotationOfExact(target)) {
    final schema = schemaName(target.name!);

    return _Type(
      kind: 'ObjectKind($schema)',
      write: (value) => 'sink.object($value, $schema)',
      read: 'source.object($schema)',
      readNullable: 'source.objectOrNull($schema)',
      field: '${target.name}Fields',
      fieldIsGenerated: true,
    );
  }

  throw InvalidGenerationSourceError(
    '`$base` is not a type the database stores: bool, int, double, String, '
    'Uint8List, a List of those or of links, a Link to a collection, or a '
    'class annotated `@Embedded()`.',
    element: where,
  );
}

String _string(String value) =>
    "'${value.replaceAll(r'\', r'\\').replaceAll("'", r"\'").replaceAll(r'$', r'\$')}'";

/// The `FieldSpec` of [field].
String _spec(_Field field) {
  final named = [
    if (field.optional) 'optional: true',
    if (field.defaultCode != null) 'defaultValue: ${field.defaultCode}',
    if (field.index && !field.unique) 'index: true',
    if (field.unique) 'unique: true',
    if (field.primaryKey) 'primaryKey: true',
  ];

  return 'FieldSpec(${[_string(field.slotName), field.type.kind, ...named].join(', ')})';
}

/// The functions that write and read the class, which both kinds share.
String _codec(String className, List<_Field> slots) {
  final writes = StringBuffer();
  final reads = StringBuffer();
  final variables = StringBuffer();

  for (final (slot, field) in slots.indexed) {
    final nonNull = field.dartType.getDisplayString().replaceAll(
      RegExp(r'\?$'),
      '',
    );

    writes
      ..writeln('    case $slot:')
      ..writeln('      ${field.type.write('object.${field.name}')};');
    variables.writeln('  $nonNull? field$slot;');
    reads
      ..writeln('      case $slot:')
      ..writeln(
        '        field$slot = '
        '${field.optional ? field.type.readNullable : field.type.read};',
      );
  }

  return '''
void _\$write$className($className object, int slot, FieldSink sink) {
  switch (slot) {
$writes  }
}

$className _\$read$className(FieldSource source) {
$variables
  while (source.next()) {
    switch (source.slot) {
$reads    }
  }

  return ${_construct(className, slots, (slot, field) => field.optional ? 'field$slot' : 'field$slot!')};
}
''';
}

/// A call of the unnamed constructor of [className] with the value
/// [value] gives each field, by position or by name as the constructor takes
/// it.
String _construct(
  String className,
  List<_Field> fields,
  String Function(int slot, _Field field) value,
) {
  final element = fields.isEmpty
      ? null
      : fields.first.element.enclosingElement as ClassElement;
  final parameters = element?.unnamedConstructor?.formalParameters ?? const [];
  final arguments = <String>[];

  for (final parameter in parameters) {
    final slot = fields.indexWhere((field) => field.name == parameter.name);

    if (slot < 0) {
      if (parameter.isRequired) {
        throw InvalidGenerationSourceError(
          "`$className`'s constructor takes `${parameter.name}`, which is "
          'not a field.',
          element: parameter,
        );
      }

      continue;
    }

    final argument = value(slot, fields[slot]);

    arguments.add(
      parameter.isNamed ? '${parameter.name}: $argument' : argument,
    );
  }

  return '$className(${arguments.join(', ')})';
}

/// The query fields of [fields]: getters at the root, or under a path for an
/// embedded object or a link's target.
String _fieldGetters(List<_Field> fields, {required bool root}) {
  final getters = StringBuffer();

  for (final field in fields) {
    final path = root
        ? "[${_string(field.slotName)}]"
        : '[...path, ${_string(field.slotName)}]';
    final made = root
        ? 'const ${field.type.field}($path)'
        : '${field.type.field}($path)';

    getters.writeln('  ${field.type.field} get ${field.name} => $made;');
  }

  return getters.toString();
}

/// The extension that gives the class a `copyWith`.
String _copyWith(String className, List<_Field> fields) {
  final parameters = [
    for (final field in fields)
      '${field.dartType.getDisplayString().replaceAll(RegExp(r'\?$'), '')}? ${field.name}',
  ];

  return '''
/// Copies of [$className] with some fields changed.
extension ${className}CopyWith on $className {
  /// A copy with the fields given changed. A field left out, or given null,
  /// keeps its value.
  $className copyWith({${parameters.join(', ')}}) => ${_construct(className, fields, (_, field) => '${field.name} ?? this.${field.name}')};
}
''';
}

final class _Collection {
  _Collection(this.element, this.collection);

  final ClassElement element;
  final String collection;

  String code() {
    final className = element.name!;
    final fields = _fieldsOf(element, embedded: false);
    final keys = fields.where((field) => field.primaryKey).toList();

    if (keys.length > 1) {
      throw InvalidGenerationSourceError(
        '`$className` has more than one `@PrimaryKey()`.',
        element: keys[1].element,
      );
    }

    final key = keys.singleOrNull;
    final List<_Field> slots;
    final String keyType;

    if (key == null) {
      final id = fields.where((field) => field.slotName == 'id').singleOrNull;

      if (id == null ||
          !id.dartType.isDartCoreInt ||
          !id.optional ||
          id.index ||
          id.unique ||
          id.defaultCode != null) {
        throw InvalidGenerationSourceError(
          'Without a `@PrimaryKey()` field, `$className` is keyed by an '
          'auto-increment, which needs a field `final int? id`, with no '
          'other annotation.',
          element: id?.element ?? element,
        );
      }

      slots = [id, ...fields.where((field) => !identical(field, id))];
      keyType = 'int';
    } else {
      if (key.optional || key.defaultCode != null) {
        throw InvalidGenerationSourceError(
          'The primary key `$className.${key.name}` is required and has no '
          'default.',
          element: key.element,
        );
      }

      if (!(key.dartType.isDartCoreInt ||
          key.dartType.isDartCoreString ||
          _isUint8List(key.dartType))) {
        throw InvalidGenerationSourceError(
          'A primary key is an int, a String or a Uint8List.',
          element: key.element,
        );
      }

      slots = fields;
      keyType = key.dartType.getDisplayString();
    }

    final schema = schemaName(className);
    final specs = [
      for (final field in slots)
        // The auto-increment `id` is declared plain: the engine knows it.
        key == null && identical(field, slots.first)
            ? "FieldSpec('id', IntKind())"
            : _spec(field),
    ];

    return '''
/// The collection `$collection` of [$className] objects, for `Schema` and
/// `txn.collection`.
const $schema = CollectionSchema<$className, ${className}Query, $keyType>(
  name: ${_string(collection)},
  autoKey: ${key == null},
  fields: [
${specs.map((spec) => '    $spec,').join('\n')}
  ],
  writeField: _\$write$className,
  read: _\$read$className,
  query: ${className}Query.new,
);

${_codec(className, slots)}
/// A query on the collection `$collection`, with a field for each field of
/// [$className].
final class ${className}Query extends QueryBuilder<$className> {
  ${className}Query();

${_fieldGetters(slots, root: true)}}

/// A link to an object of [$className], whose fields a query reads through
/// the link.
final class ${className}Link extends LinkField {
  const ${className}Link(super.path);

${_fieldGetters(slots, root: false)}}

${_copyWith(className, slots)}''';
  }
}

final class _Embedded {
  _Embedded(this.element);

  final ClassElement element;

  String code() {
    final className = element.name!;
    final fields = _fieldsOf(element, embedded: true);
    final schema = schemaName(className);

    return '''
/// The fields of the embedded object [$className].
const $schema = EmbeddedSchema<$className>(
  fields: [
${fields.map((field) => '    ${_spec(field)},').join('\n')}
  ],
  writeField: _\$write$className,
  read: _\$read$className,
);

${_codec(className, fields)}
/// The fields of the embedded object [$className] inside the object that
/// holds it, for a query.
final class ${className}Fields extends EmbeddedField<$className> {
  const ${className}Fields(List<String> path) : super(path, $schema);

${_fieldGetters(fields, root: false)}}

${_copyWith(className, fields)}''';
  }
}
