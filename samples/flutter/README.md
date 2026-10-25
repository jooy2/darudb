# DaruDB sample for Flutter

The sample app's screens in Flutter with [Plass UI](https://plass.cdget.com), for macOS, Windows, Linux, iOS and Android. [The samples' README](../README.md) says what the app does, how the sample data is made, and what you need.

| Path                  | What it is                                                                                                               |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `lib/src/model.dart`  | The annotated classes of the three collections; `model.g.dart` is generated                                              |
| `lib/src/sample.dart` | The sample data, and the seeded generator in `random.dart`                                                               |
| `lib/src/store.dart`  | Every operation the screens ask of the database                                                                          |
| `lib/src/ui/`         | The screens                                                                                                              |
| `test/`               | Unit tests of the sample data and the store                                                                              |
| `integration_test/`   | The end-to-end scenario, with or without a window or on a phone, and `engine_test.dart`, the engine on the device itself |

```bash
flutter pub get
dart run build_runner build
flutter run -d macos
```

The app depends on the `darudb` package in this checkout, whose build hook compiles the engine for the target, so the first build takes a minute or two and needs Rust. `model.g.dart` is not committed, so that the app always runs the generator in this checkout: run `build_runner` again after changing `model.dart` or the generator.

```bash
flutter test
flutter test integration_test -d flutter-tester
```

The second runs the app through the scenario with no window on the screen, at the window's size and again at a phone's; `-d macos`, `-d windows` or `-d linux` runs it in the app's window instead, and the name of a phone, a simulator or an emulator, from `flutter devices`, runs it on that device's screen. On iOS the sample needs iOS 15 or later, the oldest system the current Xcode builds for.
