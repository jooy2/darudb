// One collection's objects, a page at a time. The filter is a condition in
// the query language, and sorting and paging are the query's too: the table
// asks for `filter SORT BY field LIMIT 50 OFFSET n` and draws what comes back,
// so the engine does the work whatever the collection holds.
import 'package:darudb/darudb.dart';
import 'package:flutter/widgets.dart';
import 'package:plass_ui/plass_ui.dart';

import 'package:darudb_sample/src/fields.dart';
import 'package:darudb_sample/src/store.dart';
import 'package:darudb_sample/src/ui/cells.dart';
import 'package:darudb_sample/src/ui/controller.dart';
import 'package:darudb_sample/src/ui/format.dart';
import 'package:darudb_sample/src/ui/object_form.dart';

const int _pageSize = 50;

/// A filter for each collection, shown in the empty field as an example.
const Map<SampleCollection, String> _filterExamples =
    <SampleCollection, String>{
      SampleCollection.organizations: 'kind == "school" AND founded >= 2000',
      SampleCollection.people: 'age >= 65 AND language == "ko"',
      SampleCollection.posts: 'likes > 300 AND author.language == "en"',
    };

/// The widths of the columns that hold short values; the rest share the room.
const Map<String, double> _widths = <String, double>{
  'id': 72,
  'color': 80,
  'age': 80,
  'active': 72,
  'pinned': 72,
  'likes': 72,
  'founded': 88,
  'language': 96,
};

class CollectionView extends StatefulWidget {
  const CollectionView({super.key, required this.controller});

  final SampleController controller;

  @override
  State<CollectionView> createState() => _CollectionViewState();
}

class _CollectionViewState extends State<CollectionView> {
  final TextEditingController _filterInput = TextEditingController();
  String _filter = '';
  PlDataTableSort? _sort;
  int _page = 1;
  ListResult _result = const ListResult(<SampleRow>[], 0);
  bool _loading = true;
  DaruException? _failure;
  SampleRow? _editing;
  bool _adding = false;
  int _version = -1;
  int _request = 0;

  SampleCollection get _collection => widget.controller.collection;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onControllerChanged);
    // The first page is loading already, so nothing is set as it starts.
    _fetch();
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onControllerChanged);
    _filterInput.dispose();
    super.dispose();
  }

  void _onControllerChanged() {
    if (widget.controller.version != _version) {
      _load();
    }
  }

  /// Reads the page again, with the table showing that it is.
  void _load() {
    setState(() => _loading = true);
    _fetch();
  }

  Future<void> _fetch() async {
    final int request = ++_request;

    _version = widget.controller.version;

    try {
      final PlDataTableSort? sort = _sort;
      final ListResult result = await widget.controller.store.list(
        collection: _collection,
        filter: _filter,
        sort: sort == null
            ? null
            : SampleSort(
                sort.key,
                descending: sort.direction == PlassSortDirection.desc,
              ),
        offset: (_page - 1) * _pageSize,
        limit: _pageSize,
      );

      if (mounted && request == _request) {
        setState(() {
          _result = result;
          _failure = null;
        });
      }
    } on DaruException catch (error) {
      if (mounted && request == _request) {
        setState(() {
          _result = const ListResult(<SampleRow>[], 0);
          _failure = error;
        });
      }
    } finally {
      if (mounted && request == _request) {
        setState(() => _loading = false);
      }
    }
  }

  void _handleApplyPressed() {
    _filter = _filterInput.text.trim();
    _page = 1;
    _load();
  }

  void _handleClearPressed() {
    _filterInput.clear();
    _filter = '';
    _page = 1;
    _load();
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

  Future<void> _handleDeletePressed(SampleRow row) async {
    final Object? key = row[_collection.info.key];

    if (key == null) {
      return;
    }

    final bool confirmed = await PlConfirmProvider.of(context).confirm(
      PlConfirmOptions(
        title: Text('Delete ${_collection.name} $key?'),
        description: const Text('Objects that link to it keep its key.'),
        confirmLabel: const Text('Delete'),
        color: PlassColor.danger,
      ),
    );

    if (!confirmed || !mounted) {
      return;
    }

    try {
      await widget.controller.store.remove(_collection, key);

      if (!mounted) {
        return;
      }

      PlToastProvider.of(context).show(
        PlToast(
          color: PlassColor.success,
          title: Text('Deleted ${_collection.name} $key'),
        ),
      );
      widget.controller.changed();
    } on DaruException catch (error) {
      _showError(error);
    }
  }

  void _onSaved(Object key, {required bool inserted}) {
    setState(() {
      _editing = null;
      _adding = false;
    });
    PlToastProvider.of(context).show(
      PlToast(
        color: PlassColor.success,
        title: Text(
          '${inserted ? 'Inserted' : 'Updated'} ${_collection.name} $key',
        ),
      ),
    );
    widget.controller.changed();
  }

  List<PlDataTableColumn<SampleRow>> _getColumns() {
    final CollectionInfo info = _collection.info;

    return <PlDataTableColumn<SampleRow>>[
      if (info.autoKey)
        PlDataTableColumn<SampleRow>(
          key: info.key,
          header: Text(info.key),
          width: _widths[info.key],
          sortable: true,
          cell: (SampleRow row, int index) => cellOf(
            context,
            const FieldInfo('id', FieldKind.integer),
            row[info.key],
          ),
        ),
      for (final FieldInfo field in info.fields)
        if (field.column)
          PlDataTableColumn<SampleRow>(
            key: field.name,
            header: Text(field.name),
            width: _widths[field.name],
            sortable: info.sortable.contains(field.name),
            cell: (SampleRow row, int index) =>
                cellOf(context, field, row[field.name]),
          ),
      PlDataTableColumn<SampleRow>(
        key: 'actions',
        header: const SizedBox.shrink(),
        width: 156,
        cell: (SampleRow row, int index) => Row(
          spacing: 4,
          children: <Widget>[
            PlButton(
              size: PlassSize.xs,
              variant: PlassVariant.glass,
              onPressed: () => setState(() => _editing = row),
              child: const Text('Edit'),
            ),
            PlButton(
              size: PlassSize.xs,
              variant: PlassVariant.glass,
              color: PlassColor.danger,
              onPressed: () => _handleDeletePressed(row),
              child: const Text('Delete'),
            ),
          ],
        ),
      ),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final CollectionInfo info = _collection.info;
    final DaruException? failure = _failure;
    final SampleRow? editing = _editing;
    final Widget? form = editing != null || _adding
        ? ObjectForm(
            store: widget.controller.store,
            collection: _collection,
            row: editing,
            onClose: () => setState(() {
              _editing = null;
              _adding = false;
            }),
            onSaved: _onSaved,
          )
        : null;

    return Stack(
      children: <Widget>[
        Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            spacing: 16,
            children: <Widget>[
              Row(
                children: <Widget>[
                  PlTypography(_collection.name, level: PlTypographyLevel.h3),
                  const SizedBox(width: 12),
                  Text(
                    formatCount(_result.total),
                    key: const Key('total'),
                    style: TextStyle(
                      fontSize: 20,
                      color: PlassTheme.of(context).mutedFg,
                    ),
                  ),
                  const Spacer(),
                  PlButton(
                    onPressed: () => setState(() => _adding = true),
                    child: const Text('Add object'),
                  ),
                ],
              ),
              Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                spacing: 8,
                children: <Widget>[
                  Expanded(
                    child: PlTextField(
                      key: const Key('filter'),
                      controller: _filterInput,
                      label: const Text('Filter'),
                      placeholder: _filterExamples[_collection],
                      description: const Text(
                        'A condition in the query language. Leave it empty for every object.',
                      ),
                      onSubmitted: (_) => _handleApplyPressed(),
                      fullWidth: true,
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(top: 26),
                    child: PlButton(
                      onPressed: _handleApplyPressed,
                      child: const Text('Apply'),
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.only(top: 26),
                    child: PlButton(
                      variant: PlassVariant.ghost,
                      onPressed: _handleClearPressed,
                      child: const Text('Clear'),
                    ),
                  ),
                ],
              ),
              if (failure != null)
                PlAlert(
                  key: const Key('list-failure'),
                  color: PlassColor.danger,
                  title: Text(failure.code),
                  child: Text(failure.message),
                ),
              Flexible(
                child: PlDataTable<SampleRow>(
                  semanticLabel: 'Objects of ${_collection.name}',
                  columns: _getColumns(),
                  rows: _result.rows,
                  rowKey: (SampleRow row, int index) => row[info.key] ?? index,
                  manual: const <PlDataTableStage>[
                    PlDataTableStage.sort,
                    PlDataTableStage.pages,
                  ],
                  paging: PlDataTablePaging.pages,
                  pageSize: _pageSize,
                  page: _page,
                  onPageChanged: (int page) {
                    _page = page;
                    _load();
                  },
                  rowCount: _result.total,
                  sort: _sort,
                  onSortChanged: (PlDataTableSort? sort) {
                    _sort = sort;
                    _page = 1;
                    _load();
                  },
                  loading: _loading,
                  striped: true,
                  hoverable: true,
                  empty: const PlEmpty(
                    title: Text('No objects'),
                    description: Text(
                      'Insert sample data from the sidebar, or add an object.',
                    ),
                  ),
                ),
              ),
            ],
          ),
        ),
        ?form,
      ],
    );
  }
}
