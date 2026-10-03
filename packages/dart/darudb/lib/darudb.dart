/// DaruDB, an embedded database that keeps an application's data in one
/// local file.
///
/// Declare a class with `@Collection()`, run `build_runner` to generate its
/// schema constant, and open a database with a [Schema] of them:
///
/// ```dart
/// import 'package:darudb/darudb.dart';
///
/// part 'user.g.dart';
///
/// @Collection('users')
/// class User {
///   const User({this.id, required this.name, this.age = 0});
///
///   final int? id;
///   final String name;
///   @Index()
///   final int age;
/// }
///
/// void main() {
///   final db = Database.open('app.darudb', schema: Schema(1, [userSchema]));
///
///   db.write((txn) => txn.collection(userSchema).insert(const User(name: 'Ada')));
///
///   final adults = db.read(
///     (txn) => txn.collection(userSchema).find((q) => q.where(q.age.atLeast(18))),
///   );
///
///   db.close();
/// }
/// ```
library;

export 'src/annotations.dart';
export 'src/database.dart'
    show
        Database,
        Durability,
        Migration,
        MigrationContext,
        PasswordHashing,
        Prepared,
        ReadCollection,
        ReadTransaction,
        WriteCollection,
        WriteTransaction,
        engineVersion,
        formatVersion;
export 'src/errors.dart';
export 'src/query.dart'
    show
        BoolField,
        BytesField,
        Change,
        Condition,
        EmbeddedField,
        Field,
        FloatField,
        IntField,
        LinkField,
        ListField,
        OrderedField,
        QueryBuilder,
        StringField,
        StringListField,
        ValueField;
export 'src/schema.dart'
    show
        BoolKind,
        BytesKind,
        CollectionSchema,
        EmbeddedSchema,
        FieldSink,
        FieldSource,
        FieldSpec,
        FloatKind,
        IntKind,
        Kind,
        Link,
        LinkKind,
        ListKind,
        ObjectKind,
        Schema,
        StringKind;
