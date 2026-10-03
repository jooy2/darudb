---
title: Constants
order: 16
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

The file format version this build of the engine reads and writes. Every file records the version it was written in, which [`Database.formatVersion`](../../api/node/database.md) reads, and opening a file with another version fails with `UNSUPPORTED_FORMAT_VERSION`. The format is not stable before the first release, so a file that an older build wrote can fail this way after an upgrade. [File format](../../engine/file-format.md) describes what the version covers.

## engineVersion

```ts
const engineVersion: () => string;
```

Returns the version of the DaruDB engine inside the package, such as `'0.1.0'`. The engine and the npm package are versioned separately, so it can differ from the version in the package's `package.json`.
