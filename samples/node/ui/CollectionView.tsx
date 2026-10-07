/**
 * One collection's objects, a page at a time. The filter is a condition in
 * the query language, and sorting and paging are the query's too: the
 * table asks for `filter SORT BY field LIMIT 50 OFFSET n` and draws what
 * comes back, so the engine does the work whatever the collection holds.
 */
import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';

import { PlAlert } from 'plass-ui/alert';
import { PlButton } from 'plass-ui/button';
import { usePlConfirm } from 'plass-ui/confirm';
import { PlDataTable } from 'plass-ui/data-table';
import type { PlDataTableColumn, PlDataTableSort } from 'plass-ui/data-table';
import { PlEmpty } from 'plass-ui/empty';
import { PlFlex } from 'plass-ui/flex';
import { PlTextField } from 'plass-ui/text-field';
import { usePlToast } from 'plass-ui/toast';
import { PlTypography } from 'plass-ui/typography';

import { COLLECTIONS } from '../core/fields.ts';
import type { CollectionName } from '../core/fields.ts';
import type { Key, ListResult, WireObject } from '../core/protocol.ts';

import { backend, describeError } from './backend.ts';
import { renderCell } from './cells.tsx';
import { formatCount } from './format.ts';
import { ObjectForm } from './ObjectForm.tsx';

const PAGE_SIZE = 50;

/** A filter for each collection, shown in the empty field as an example. */
const FILTER_EXAMPLES: Record<CollectionName, string> = {
  organizations: 'kind == "school" AND founded >= 2000',
  people: 'age >= 65 AND language == "ko"',
  posts: 'likes > 300 AND author.language == "en"'
};

interface CollectionViewProps {
  collection: CollectionName;
  /** Changes whenever the file may have changed, so the page is read again. */
  version: number;
  onChanged: () => void;
}

export const CollectionView = ({ collection, version, onChanged }: CollectionViewProps) => {
  const info = COLLECTIONS[collection];
  const toast = usePlToast();
  const { confirm } = usePlConfirm();
  const [filterText, setFilterText] = useState('');
  const [filter, setFilter] = useState('');
  const [sort, setSort] = useState<PlDataTableSort | null>(null);
  const [page, setPage] = useState(1);
  const [result, setResult] = useState<ListResult>({ objects: [], total: 0 });
  const [loading, setLoading] = useState(true);
  const [failure, setFailure] = useState<{ code: string; message: string } | null>(null);
  const [editing, setEditing] = useState<{ object: WireObject | null } | null>(null);

  useEffect(() => {
    let current = true;

    setLoading(true);
    backend
      .call('list', {
        collection,
        filter,
        sort: sort === null ? null : { field: sort.key, direction: sort.direction },
        offset: (page - 1) * PAGE_SIZE,
        limit: PAGE_SIZE
      })
      .then(
        (next) => {
          if (current) {
            setResult(next);
            setFailure(null);
          }
        },
        (error: unknown) => {
          if (current) {
            setResult({ objects: [], total: 0 });
            setFailure(describeError(error));
          }
        }
      )
      .finally(() => {
        if (current) {
          setLoading(false);
        }
      });

    return () => {
      current = false;
    };
  }, [collection, filter, sort, page, version]);

  const handleFilter = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    setFilter(filterText.trim());
    setPage(1);
  };

  const handleClear = (): void => {
    setFilterText('');
    setFilter('');
    setPage(1);
  };

  const handleDelete = async (object: WireObject): Promise<void> => {
    const key = object[info.key] as Key;
    const confirmed = await confirm({
      title: `Delete ${collection} ${key}?`,
      description: 'Objects that link to it keep its key.',
      confirmLabel: 'Delete',
      color: 'danger'
    });

    if (!confirmed) {
      return;
    }

    try {
      await backend.call('remove', { collection, key });
      toast.add({ title: `Deleted ${collection} ${key}`, color: 'success' });
      onChanged();
    } catch (error) {
      const { code, message } = describeError(error);

      toast.add({ title: code, description: message, color: 'danger' });
    }
  };

  const handleSaved = (key: Key, inserted: boolean): void => {
    setEditing(null);
    toast.add({
      title: `${inserted ? 'Inserted' : 'Updated'} ${collection} ${key}`,
      color: 'success'
    });
    onChanged();
  };

  const columns: PlDataTableColumn<WireObject>[] = [
    ...(info.autoKey
      ? [
          {
            key: info.key,
            header: info.key,
            sortable: true,
            width: 90,
            render: (object: WireObject) => String(object[info.key])
          }
        ]
      : []),
    ...info.fields
      .filter((field) => field.column)
      .map((field) => ({
        key: field.name,
        header: field.name,
        sortable: info.sortable.includes(field.name),
        render: (object: WireObject) => renderCell(field, object[field.name])
      })),
    {
      key: 'actions',
      header: '',
      unsearchable: true,
      width: 150,
      render: (object: WireObject) => (
        <PlFlex spacing={1}>
          <PlButton size="xs" variant="glass" onClick={() => setEditing({ object })}>
            Edit
          </PlButton>
          <PlButton
            size="xs"
            variant="glass"
            color="danger"
            onClick={() => void handleDelete(object)}
          >
            Delete
          </PlButton>
        </PlFlex>
      )
    }
  ];

  return (
    <section className="sample-collection" aria-label={collection}>
      <PlFlex justify="space-between" alignItems="center">
        <PlTypography level="h2" headingLevel={2}>
          {collection}{' '}
          <span className="sample-muted" data-testid="total">
            {formatCount(result.total)}
          </span>
        </PlTypography>
        <PlButton onClick={() => setEditing({ object: null })}>Add object</PlButton>
      </PlFlex>
      <form className="sample-filter" onSubmit={handleFilter}>
        <PlTextField
          label="Filter"
          name="filter"
          placeholder={FILTER_EXAMPLES[collection]}
          description="A condition in the query language. Leave it empty for every object."
          value={filterText}
          onChange={(event) => setFilterText(event.target.value)}
          fullWidth
        />
        <PlButton type="submit">Apply</PlButton>
        <PlButton type="button" variant="ghost" onClick={handleClear}>
          Clear
        </PlButton>
      </form>
      {failure === null ? null : (
        <PlAlert color="danger" title={failure.code}>
          {failure.message}
        </PlAlert>
      )}
      <PlDataTable<WireObject>
        label={`Objects of ${collection}`}
        columns={columns}
        rows={result.objects}
        getRowKey={(object) => String(object[info.key])}
        manual={['sort', 'pages']}
        paging="pages"
        pageSize={PAGE_SIZE}
        page={page}
        onPageChange={setPage}
        rowCount={result.total}
        sort={sort}
        onSortChange={(next) => {
          setSort(next);
          setPage(1);
        }}
        loading={loading}
        striped
        hoverable
        empty={
          <PlEmpty
            title="No objects"
            description="Insert sample data from the sidebar, or add an object."
          />
        }
      />
      {editing === null ? null : (
        <ObjectForm
          collection={collection}
          object={editing.object}
          onClose={() => setEditing(null)}
          onSaved={handleSaved}
        />
      )}
    </section>
  );
};
