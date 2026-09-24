'use strict';

/**
 * DaruDB for Node.js: an embedded database that keeps an application's data
 * in one local file.
 *
 * `index.d.ts` beside this file declares the API, with the types a schema
 * gives the objects of each collection.
 */

const native = require('./native.js');
const { t, collection, schema } = require('./lib/schema');
const { Query, conditions } = require('./lib/query');
const { Database } = require('./lib/database');

exports.Database = Database;
exports.t = t;
exports.collection = collection;
exports.schema = schema;
exports.Query = Query;
exports.conditions = conditions;
exports.engineVersion = native.engineVersion;
exports.FORMAT_VERSION = native.FORMAT_VERSION;
