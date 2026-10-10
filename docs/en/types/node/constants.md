---
title: Constants
order: 17
group: errors
pageClass: reference-page
---

# Constants

The package exports two values besides its API: the newest file format version this build reads and writes, and the version of the engine inside it.

```ts
import { engineVersion, FORMAT_VERSION } from 'darudb';

console.log(`DaruDB engine ${engineVersion()}, file format ${FORMAT_VERSION}`);
```

## FORMAT_VERSION

```ts
const FORMAT_VERSION: number;
```

The newest file format version this build of the engine reads and writes, 6, which a new file gets. Every file records the version it was written in, which [`Database.formatVersion`](../../api/node/database.md#formatversion) reads, and opening a file in a version this build does not read fails with `UNSUPPORTED_FORMAT_VERSION`. Version 5 is the format of the first release, and version 6 writes the lengths in a leaf's entries in fewer bytes. This build reads and writes both, and raises a file of version 5 to version 6 when it opens it, unless [`upgradeFormat`](./open-options.md#upgradeformat) is `false`; a file a development build wrote before version 5 does not open. [File format](../../engine/file-format.md#format-versions) describes what the version covers.

## engineVersion

```ts
const engineVersion: () => string;
```

Returns the version of the DaruDB engine inside the package, such as `'1.1.0'`. The engine and the npm package are versioned separately, so it can differ from the version in the package's `package.json`.
