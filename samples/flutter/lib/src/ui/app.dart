// The app: Plass's providers for toasts and confirmations, the backdrop its
// glass needs, and the one screen.
import 'package:flutter/material.dart';
import 'package:plass_ui/plass_ui.dart';

import 'package:darudb_sample/src/ui/home_page.dart';

class SampleApp extends StatelessWidget {
  const SampleApp({super.key, this.directory});

  /// Where the database file goes; see [HomePage.directory].
  final String? directory;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'DaruDB Sample',
      debugShowCheckedModeBanner: false,
      // The confirmations need an overlay, and the builder sits above the
      // navigator's, so they get one of their own.
      builder: (BuildContext context, Widget? child) => _Page(
        child: PlToastProvider(
          child: Overlay.wrap(
            child: PlConfirmProvider(child: child ?? const SizedBox.shrink()),
          ),
        ),
      ),
      home: HomePage(directory: directory),
    );
  }
}

/// The page under the components: Plass draws glass, which needs something
/// behind it, and leaves the page's own background and text style to the app.
/// It wraps the toasts and the confirmations too, which sit above the screen.
class _Page extends StatelessWidget {
  const _Page({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    final PlassTokens tokens = PlassTheme.of(context);

    return Material(
      type: MaterialType.transparency,
      child: DecoratedBox(
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topCenter,
            end: Alignment.bottomCenter,
            colors: <Color>[tokens.bgFrom, tokens.bgTo],
          ),
        ),
        child: DefaultTextStyle.merge(
          style: TextStyle(color: tokens.fg, fontSize: 14),
          child: child,
        ),
      ),
    );
  }
}
