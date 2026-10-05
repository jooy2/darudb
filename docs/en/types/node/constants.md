---
title: Constants
order: 17
---

# Constants

The package exports two values besides its API: the file format version this build reads and writes, and the version of the engine inside it.

```ts
import { engineVersion, FORMAT_VERSION } from 'darudb';

console.log(`DaruDB engine ${engineVersion()}, file format ${FORMAT_VERSION}`);
```

## FORMAT_VERSION

```ts
const FORMAT_VERSION: number;
```

The file format version this build of the engine reads and writes. Every file records the version it was written in, which [`Database.formatVersion`](../../api/node/database.md) reads, and opening a file with another version fails with `UNSUPPORTED_FORMAT_VERSION`. Version 5 is the format of the first release: a file a development build wrote before it fails this way, and every later format version will come with a migration from the one before. [File format](../../engine/file-format.md) describes what the version covers.

## engineVersion

```ts
const engineVersion: () => string;
```

Returns the version of the DaruDB engine inside the package, such as `'0.1.0'`. The engine and the npm package are versioned separately, so it can differ from the version in the package's `package.json`.
