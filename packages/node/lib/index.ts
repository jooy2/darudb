/**
 * DaruDB for Node.js: an embedded database that keeps an application's data
 * in one local file.
 *
 * `index.d.ts` at the package's root declares the API, with the types a
 * schema gives the objects of each collection.
 */

// Loaded with `require` rather than named imports: `tsc` makes a name
// imported and exported again a getter on `exports`, and these stay values.
import native = require('../native.js');
import schemaModule = require('./schema.js');
import queryModule = require('./query.js');
import databaseModule = require('./database.js');

export const Database = databaseModule.Database;
export const t = schemaModule.t;
export const collection = schemaModule.collection;
export const schema = schemaModule.schema;
export const Query = queryModule.Query;
export const conditions = queryModule.conditions;
export const param = queryModule.param;
export const engineVersion = native.engineVersion;
export const FORMAT_VERSION = native.FORMAT_VERSION;
