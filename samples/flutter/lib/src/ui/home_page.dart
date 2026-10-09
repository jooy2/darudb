// The sample's one screen: the collections and the sample data in the
// sidebar, the chosen collection's objects beside them, and the file's tools
// in the header. It opens the store when it first appears and closes it when
// it goes.
import 'dart:async';

import 'package:darudb/darudb.dart';
import 'package:flutter/widgets.dart';
import 'package:path_provider/path_provider.dart';
import 'package:plass_ui/plass_ui.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/store.dart';
import 'package:darudb_sample/src/ui/collection_view.dart';
import 'package:darudb_sample/src/ui/controller.dart';
import 'package:darudb_sample/src/ui/format.dart';
import 'package:darudb_sample/src/ui/sample_panel.dart';

/// The key a test reads a collection's count by.
Key countKey(SampleCollection collection) => Key('count-${collection.name}');

class HomePage extends StatefulWidget {
  const HomePage({super.key, this.directory});

  /// The folder the database file goes in: a `data` folder in the app's
  /// support folder when this is `null`.
  final String? directory;

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  SampleController? _controller;
  Object? _openFailure;

  @override
  void initState() {
    super.initState();
    _open();
  }

  @override
  void dispose() {
    final SampleController? controller = _controller;

    if (controller != null) {
      controller.dispose();
      unawaited(controller.store.close());
    }

    super.dispose();
  }

  Future<void> _open() async {
    try {
      final String directory =
          widget.directory ??
          '${(await getApplicationSupportDirectory()).path}/data';
      final SampleStore store = await SampleStore.open(directory);
      final SampleController controller = SampleController(store);

      await controller.refresh();

      if (!mounted) {
        controller.dispose();
        await store.close();

        return;
      }

      setState(() => _controller = controller);
    } on Object catch (error) {
      if (mounted) {
        setState(() => _openFailure = error);
      }
    }
  }

  void _showError(DaruException error) {
    PlToastProvider.of(context).show(
      PlToast(
        color: PlassColor.danger,
        title: Text(error.code),
        description: Text(error.message),
      ),
    );
  }

  void _showDone(String title, [String? description]) {
    PlToastProvider.of(context).show(
      PlToast(
        color: PlassColor.success,
        title: Text(title),
        description: description == null ? null : Text(description),
      ),
    );
  }

  Future<void> _handleCheckPressed(SampleController controller) async {
    try {
      final CheckSummary report = await controller.runTool(
        controller.store.check,
      );

      if (!mounted) {
        return;
      }

      final String checked =
          '${formatCount(report.pagesChecked)} pages and '
          '${formatCount(report.objectsChecked)} objects checked.';

      if (report.ok) {
        _showDone('The file is intact', checked);
      } else {
        PlToastProvider.of(context).show(
          PlToast(
            color: PlassColor.danger,
            title: Text('${report.problems.length} problems found'),
            description: Text('$checked ${report.problems.first}'),
          ),
        );
      }
    } on DaruException catch (error) {
      _showError(error);
    }
  }

  Future<void> _handleCompactPressed(SampleController controller) async {
    try {
      final CompactReport report = await controller.runTool(
        controller.store.compact,
      );

      if (mounted) {
        _showDone(
          'Compacted',
          'From ${formatBytes(report.bytesBefore)} to '
              '${formatBytes(report.bytesAfter)}, '
              '${formatCount(report.pagesMoved)} pages moved.',
        );
      }
    } on DaruException catch (error) {
      _showError(error);
    }
  }

  Future<void> _handleResetPressed(SampleController controller) async {
    final bool confirmed = await PlConfirmProvider.of(context).confirm(
      const PlConfirmOptions(
        title: Text('Delete every object?'),
        description: Text(
          'The database file is deleted and made again, empty.',
        ),
        confirmLabel: Text('Reset'),
        color: PlassColor.danger,
      ),
    );

    if (!confirmed) {
      return;
    }

    try {
      await controller.runTool(controller.store.reset);

      if (mounted) {
        _showDone('The database is empty');
      }
    } on DaruException catch (error) {
      _showError(error);
    }
  }

  Widget _getHeaderWidget(SampleController controller) {
    return PlHeader(
      brand: const <Widget>[
        PlTypography(
          'DaruDB Sample',
          level: PlTypographyLevel.h5,
          weight: PlTypographyWeight.semibold,
        ),
        PlChip(size: PlassSize.sm, child: Text('Flutter')),
      ],
      actions: <Widget>[
        PlButton(
          variant: PlassVariant.glass,
          onPressed: controller.busy
              ? null
              : () => _handleCheckPressed(controller),
          child: const Text('Check'),
        ),
        PlButton(
          variant: PlassVariant.glass,
          onPressed: controller.busy
              ? null
              : () => _handleCompactPressed(controller),
          child: const Text('Compact'),
        ),
        PlButton(
          color: PlassColor.danger,
          onPressed: controller.busy
              ? null
              : () => _handleResetPressed(controller),
          child: const Text('Reset'),
        ),
      ],
    );
  }

  Widget _getSidebarWidget(SampleController controller) {
    final SampleInfo? info = controller.info;

    return PlSidebar(
      width: 320,
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          spacing: 16,
          children: <Widget>[
            PlList(
              children: <Widget>[
                for (final SampleCollection collection
                    in SampleCollection.values)
                  PlListItem(
                    selected: collection == controller.collection,
                    onPressed: () => controller.select(collection),
                    endIcon: Text(
                      formatCount(info?.counts[collection] ?? 0),
                      key: countKey(collection),
                    ),
                    child: Text(collection.name),
                  ),
              ],
            ),
            SamplePanel(controller: controller),
            if (info != null)
              PlCard(
                title: const Text('File'),
                child: PlDataList(
                  orientation: PlassOrientation.vertical,
                  size: PlassSize.sm,
                  children: <PlDataListItem>[
                    PlDataListItem(
                      label: const Text('Path'),
                      value: Text(info.path),
                    ),
                    PlDataListItem(
                      label: const Text('Size'),
                      value: Text(formatBytes(info.bytes)),
                    ),
                    PlDataListItem(
                      label: const Text('Format'),
                      value: Text(
                        'version ${info.formatVersion}, pages of '
                        '${formatBytes(info.pageSize)}, '
                        '${info.encrypted ? 'encrypted' : 'not encrypted'}',
                      ),
                    ),
                    PlDataListItem(
                      label: const Text('Engine'),
                      value: Text(
                        'DaruDB ${info.engineVersion}, schema version '
                        '${info.schemaVersion ?? 'none'}',
                      ),
                    ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final SampleController? controller = _controller;
    final Object? failure = _openFailure;

    if (controller == null) {
      final Widget waiting = failure == null
          ? const PlProgressCircular(label: Text('Opening the database'))
          : PlAlert(
              color: PlassColor.danger,
              title: const Text('The database did not open'),
              child: Text(failure.toString()),
            );

      return Center(child: waiting);
    }

    return ListenableBuilder(
      listenable: controller,
      builder: (BuildContext context, Widget? child) => PlPageLayout(
        collapseBelow: null,
        header: _getHeaderWidget(controller),
        sidebar: _getSidebarWidget(controller),
        child: CollectionView(
          key: ValueKey<SampleCollection>(controller.collection),
          controller: controller,
        ),
      ),
    );
  }
}
