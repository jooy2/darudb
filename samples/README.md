# Sample apps

Apps that use DaruDB the way an application does, with end-to-end tests that drive them from the user's side. The engine's and the bindings' own tests check each call on its own; these check that the pieces work together in a real app: a file that opens, a few hundred thousand objects inserted in bulk, a list that filters, sorts and pages through them, objects added, changed and deleted from a form, and the same data there after the app starts again.

Nothing here is released. The packages ship only their own folders, the documentation site is built from `docs/`, and the samples depend on the packages in this checkout rather than on the published ones, so they run and test the code as it is in the repository.

| Path      | What it is                                                                       | Tested with                          |
| --------- | -------------------------------------------------------------------------------- | ------------------------------------ |
| `node`    | One set of React screens, run as an Electron app and as a page of a local server | Playwright, in Electron and Chromium |
| `flutter` | The same screens as a Flutter app, for macOS, Windows, Linux, iOS and Android    | `integration_test`, on a desktop     |

Both use [Plass UI](https://plass.cdget.com) for their components and [randino](https://randino.cdget.com) for their sample data.

## What the apps do

The app opens a database file when it starts, empty the first time, and keeps it between runs. From there:

- **Insert sample data.** Choose 1,000, 10,000 or 100,000 people and a seed, and the app makes the people, an organization for every fifty of them and three posts each, and inserts them in write transactions of 5,000 objects. It reports the time spent writing apart from the time spent making the objects, which is not the engine's.
- **Browse a collection.** The sidebar lists the three collections with their counts. The list shows a page of 50 objects, filtered by a condition in the query language, such as `age >= 65 AND language == "ko"` or `author.language == "en"` for posts, and sorted by a column. The engine does the filtering, sorting and paging: the list asks for `filter SORT BY field LIMIT 50 OFFSET n`.
- **Add, change and delete objects.** A form for each collection, which shows the engine's error code when it refuses a change, `DUPLICATE_KEY` for a nickname another person holds, for one.
- **Check, compact and reset the file.** The integrity check, compaction, and a reset that deletes the file and starts again from an empty one.

The Node.js sample runs the same screens in two hosts. The Electron app holds the database in its main process and the window asks for everything over IPC; the web page cannot load the engine, so a server on this machine holds the database and serves the page.

## The sample data

Every run makes the same three collections, which use every kind of field the engine has:

| Collection      | Key              | Fields                                                                                                                                                                                                                              |
| --------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `organizations` | `code`, a string | `name`, `kind`, `industry` (optional), `language`, `founded`                                                                                                                                                                        |
| `people`        | `id`, assigned   | `name`, `nickname` (unique), `email` (optional, unique), `age`, `gender`, `language`, `location` (an embedded object, optional), `organization` (a link), `tags` (a list), `active`, `score` (a float), `color` (bytes), `joinedAt` |
| `posts`         | `id`, assigned   | `author` (a link to a person), `title`, `body`, `language`, `tags` (optional), `likes`, `pinned`, `createdAt`                                                                                                                       |

People come in nine languages, most of them Korean and English, with names, nicknames, places, organizations and sentences from randino. A post is in its author's language.

- **The same seed makes the same objects.** Each object draws from a seed of its own, made from the run's seed and the object's number, so person 1042 of seed 7 is the same person in every run of that seed. The two samples use the same seeded generator, but randino draws differently in each language, so they do not make the same names.
- **A second run continues the numbers.** A run starts from one past the highest organization code, person and post already in the file. The unique fields carry the number, so running the same seed twice adds new objects instead of being refused.
- **A run makes its pools first.** randino writes about four thousand full sentences a second, so a run fills pools of names, words, sentences and places for each language from the seed, and each object is a handful of draws from them.

## What you need

- **Rust**, through [rustup](https://rustup.rs). Both samples build the engine from this checkout, and rustup installs the version the repository pins on the first build.
- **Node.js 22.18 or later** for the Node.js sample, which runs its TypeScript as it is.
- **Flutter 3.41 or later** for the Flutter sample, Plass UI's minimum, with the tools for the platform you build for.

## Run the Node.js sample

Build the package first, since the sample loads the addon it builds:

```bash
cd packages/node
npm install
npm run build
```

Then, in `samples/node`:

```bash
npm install
npm start
```

`npm start` builds the screens and opens the Electron app, which keeps its database under the app's user data folder. `npm run web` serves the same screens at `http://127.0.0.1:3000`, with the database in `samples/node/.data/`. `npm run dev` serves them with hot reloading, the database included. `DARUDB_SAMPLE_DIR` puts the database in another folder, and `PORT` serves the web page on another port.

The end-to-end tests run the same scenario in both hosts. Playwright needs its Chromium once:

```bash
npx playwright install chromium
npm test
```

`npm run test:web` and `npm run test:electron` run one host, `npm run typecheck` checks the types and `npm run format` the formatting. After a change to the engine or the binding, build `packages/node` again before testing.

## Run the Flutter sample

In `samples/flutter`, generate the model's part, then run the app on a desktop or a device:

```bash
flutter pub get
dart run build_runner build
flutter run -d macos
```

The first build compiles the engine for the target, which takes a minute or two; later builds reuse it. The app keeps its database in a `data` folder of its application support folder.

```bash
flutter test
flutter test integration_test -d macos
```

`flutter test` runs the unit tests of the sample data and the store, against the engine. `flutter test integration_test` runs the app itself through the scenario, on the desktop you name: `-d macos`, `-d windows` or `-d linux`. The test drives a window, so it needs a display and a session that is not locked.

## In CI

`.github/workflows/run-test-samples.yml` runs both on Linux, on every pull request and on pushes that change the engine, a binding or a sample: the Node.js sample's tests in Electron and Chromium, and the Flutter sample's unit tests and end-to-end tests on the Linux desktop, each under a virtual display.
