/// The builder `build_runner` runs for the `darudb` package: it writes the
/// schema, the query builder and the `copyWith` of every class annotated
/// `@Collection()` or `@Embedded()` into the library's `.g.dart` part.
library;

import 'package:build/build.dart';
import 'package:source_gen/source_gen.dart';

import 'src/generator.dart';

/// The builder `build.yaml` names.
Builder darudbBuilder(BuilderOptions options) =>
    SharedPartBuilder([const DaruGenerator()], 'darudb');
