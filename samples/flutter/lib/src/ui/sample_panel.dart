// Inserting sample data: how many people, from which seed, and a progress
// bar while the run commits its batches. The report keeps the time the
// engine spent writing apart from the time spent making the objects.
import 'package:darudb/darudb.dart';
import 'package:flutter/widgets.dart';
import 'package:plass_ui/plass_ui.dart';

import 'package:darudb_sample/src/sample.dart';
import 'package:darudb_sample/src/store.dart';
import 'package:darudb_sample/src/ui/controller.dart';
import 'package:darudb_sample/src/ui/format.dart';

const List<int> _sizes = <int>[1000, 10000, 100000];

const Map<SeedStage, String> _stageLabels = <SeedStage, String>{
  SeedStage.pools: 'Preparing names and sentences',
  SeedStage.organizations: 'Inserting organizations',
  SeedStage.people: 'Inserting people',
  SeedStage.posts: 'Inserting posts',
};

class SamplePanel extends StatefulWidget {
  const SamplePanel({super.key, required this.controller});

  final SampleController controller;

  @override
  State<SamplePanel> createState() => _SamplePanelState();
}

class _SamplePanelState extends State<SamplePanel> {
  int _people = _sizes.first;
  int _seed = 1;

  /// How many objects of the run are in, by the stage that is going on.
  int _doneOf(SeedProgress progress, Plan plan) => switch (progress.stage) {
    SeedStage.pools => 0,
    SeedStage.organizations => progress.done,
    SeedStage.people => plan.organizations + progress.done,
    SeedStage.posts => plan.organizations + plan.people + progress.done,
  };

  Future<void> _handleInsertPressed() async {
    try {
      final SeedReport report = await widget.controller.seed(
        people: _people,
        seed: _seed,
      );

      if (!mounted) {
        return;
      }

      PlToastProvider.of(context).show(
        PlToast(
          color: PlassColor.success,
          title: Text('Inserted ${formatCount(report.plan.total)} objects'),
        ),
      );
    } on DaruException catch (error) {
      if (!mounted) {
        return;
      }

      PlToastProvider.of(context).show(
        PlToast(
          color: PlassColor.danger,
          title: Text(error.code),
          description: Text(error.message),
        ),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final Plan plan = Plan(_people);
    final SampleController controller = widget.controller;
    final SeedProgress? progress = controller.progress;
    final SeedReport? report = controller.report;
    final bool running = controller.busy && progress != null;

    return PlCard(
      title: const Text('Sample data'),
      subtitle: const Text(
        'People in nine languages, with organizations and posts',
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        spacing: 12,
        children: <Widget>[
          PlSegmentedButton<int>(
            semanticLabel: 'How many people',
            size: PlassSize.sm,
            fullWidth: true,
            value: _people,
            onChanged: (int value) => setState(() => _people = value),
            segments: <PlSegment<int>>[
              for (final int size in _sizes)
                PlSegment<int>(value: size, label: Text(formatCount(size))),
            ],
          ),
          Text(
            '${formatCount(plan.organizations)} organizations, '
            '${formatCount(plan.people)} people, ${formatCount(plan.posts)} posts',
            style: TextStyle(color: PlassTheme.of(context).mutedFg),
          ),
          PlNumberField(
            key: const Key('seed'),
            label: const Text('Seed'),
            description: const Text('The same seed makes the same objects.'),
            size: PlassSize.sm,
            value: _seed.toDouble(),
            min: 0,
            max: 4294967295,
            onChanged: (double? value) =>
                setState(() => _seed = value?.round() ?? 0),
            fullWidth: true,
          ),
          PlButton(
            loading: running,
            onPressed: controller.busy ? null : _handleInsertPressed,
            child: const Text('Insert sample data'),
          ),
          if (progress != null)
            PlProgressLinear(
              label: Text(_stageLabels[progress.stage] ?? ''),
              value: _doneOf(progress, plan).toDouble(),
              max: plan.total.toDouble(),
              showValue: true,
            ),
          if (report != null)
            PlDataList(
              key: const Key('report'),
              size: PlassSize.sm,
              labelWidth: 96,
              children: <PlDataListItem>[
                PlDataListItem(
                  label: const Text('Objects'),
                  value: Text(formatCount(report.plan.total)),
                ),
                PlDataListItem(
                  label: const Text('Writing'),
                  value: Text(formatDuration(report.insertMs)),
                ),
                PlDataListItem(
                  label: const Text('Rate'),
                  value: Text(formatRate(report.plan.total, report.insertMs)),
                ),
                PlDataListItem(
                  label: const Text('Generating'),
                  value: Text(formatDuration(report.generateMs)),
                ),
              ],
            ),
        ],
      ),
    );
  }
}
