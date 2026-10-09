/**
 * The sample's one screen: the collections and the sample data in the
 * sidebar, the chosen collection's objects beside them, and the file's tools
 * in the header. The file's card chooses whether the file is encrypted,
 * which makes it again, empty. Every change bumps `version`, which reads the
 * counts and the page again.
 */
import { useCallback, useEffect, useState } from 'react';

import { PlButton } from 'plass-ui/button';
import { PlCard } from 'plass-ui/card';
import { PlChip } from 'plass-ui/chip';
import { usePlConfirm } from 'plass-ui/confirm';
import { PlDataList, PlDataListItem } from 'plass-ui/data-list';
import { PlFlex } from 'plass-ui/flex';
import { PlHeader } from 'plass-ui/header';
import { PlList, PlListItem } from 'plass-ui/list';
import { PlPageLayout } from 'plass-ui/page-layout';
import { PlSegment, PlSegmentedButton } from 'plass-ui/segmented-button';
import { PlSidebar } from 'plass-ui/sidebar';
import { usePlToast } from 'plass-ui/toast';
import { PlTypography } from 'plass-ui/typography';
import { PlVisuallyHidden } from 'plass-ui/visually-hidden';

import { COLLECTION_NAMES } from '../core/fields.ts';
import type { CollectionName } from '../core/fields.ts';
import type { Info } from '../core/protocol.ts';

import { backend, describeError } from './backend.ts';
import { CollectionView } from './CollectionView.tsx';
import { formatBytes, formatCount } from './format.ts';
import { SamplePanel } from './SamplePanel.tsx';

const HOST_LABELS = { electron: 'Electron', web: 'Web' } as const;

export const App = () => {
  const toast = usePlToast();
  const { confirm } = usePlConfirm();
  const [info, setInfo] = useState<Info | null>(null);
  const [collection, setCollection] = useState<CollectionName>('people');
  const [version, setVersion] = useState(0);
  const [busy, setBusy] = useState(false);

  const showError = useCallback(
    (error: unknown) => {
      const { code, message } = describeError(error);

      toast.add({ title: code, description: message, color: 'danger' });
    },
    [toast]
  );

  useEffect(() => {
    backend.call('info', null).then(setInfo, showError);
  }, [version, showError]);

  const onChanged = (): void => {
    setVersion((current) => current + 1);
  };

  /** Runs a tool of the header with the other tools held off until it ends. */
  const runTool = async (work: () => Promise<void>): Promise<void> => {
    setBusy(true);

    try {
      await work();
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
      onChanged();
    }
  };

  const handleCheck = (): Promise<void> =>
    runTool(async () => {
      const report = await backend.call('check', null);

      toast.add({
        title: report.ok ? 'The file is intact' : `${report.problems.length} problems found`,
        description: `${formatCount(report.pagesChecked)} pages and ${formatCount(report.objectsChecked)} objects checked.${report.problems.length > 0 ? ` ${report.problems[0]}` : ''}`,
        color: report.ok ? 'success' : 'danger'
      });
    });

  const handleCompact = (): Promise<void> =>
    runTool(async () => {
      const report = await backend.call('compact', null);

      toast.add({
        title: 'Compacted',
        description: `From ${formatBytes(report.bytesBefore)} to ${formatBytes(report.bytesAfter)}, ${formatCount(report.pagesMoved)} pages moved.`,
        color: 'success'
      });
    });

  /** Makes the file again, empty, encrypted or not, once the user agrees. */
  const remake = async (encrypted: boolean, title: string, confirmLabel: string): Promise<void> => {
    const confirmed = await confirm({
      title,
      description: encrypted
        ? "The database file is deleted and made again, empty, encrypted with the sample's password."
        : 'The database file is deleted and made again, empty.',
      confirmLabel,
      color: 'danger'
    });

    if (confirmed) {
      await runTool(async () => {
        setInfo(await backend.call('reset', { encrypted }));
        toast.add({ title: 'The database is empty', color: 'success' });
      });
    }
  };

  const handleReset = (): Promise<void> =>
    remake(info?.encrypted ?? false, 'Delete every object?', 'Reset');

  const handleEncryptionChange = (encrypted: boolean): Promise<void> =>
    encrypted
      ? remake(true, 'Make an encrypted file?', 'Encrypt')
      : remake(false, 'Make a plain file?', 'Make plain');

  const header = (
    <PlHeader
      brand={
        <PlFlex alignItems="center" spacing={2}>
          <PlTypography level="h5" headingLevel={1}>
            DaruDB Sample
          </PlTypography>
          <PlChip size="sm">{HOST_LABELS[backend.host]}</PlChip>
        </PlFlex>
      }
      actions={
        <PlFlex spacing={2}>
          <PlButton variant="glass" onClick={() => void handleCheck()} disabled={busy}>
            Check
          </PlButton>
          <PlButton variant="glass" onClick={() => void handleCompact()} disabled={busy}>
            Compact
          </PlButton>
          <PlButton color="danger" onClick={() => void handleReset()} disabled={busy}>
            Reset
          </PlButton>
        </PlFlex>
      }
    />
  );

  const sidebar = (
    <PlSidebar width={320} label="Collections and sample data">
      <PlFlex direction="vertical" spacing={4}>
        <PlList aria-label="Collections">
          {COLLECTION_NAMES.map((name) => (
            <PlListItem
              key={name}
              selected={name === collection}
              onClick={() => setCollection(name)}
              endIcon={
                <span className="sample-count">
                  <span data-testid={`count-${name}`}>{formatCount(info?.counts[name] ?? 0)}</span>
                  <PlVisuallyHidden> objects</PlVisuallyHidden>
                </span>
              }
            >
              {name}
            </PlListItem>
          ))}
        </PlList>
        <SamplePanel
          disabled={busy}
          onStart={() => setBusy(true)}
          onFinish={() => {
            setBusy(false);
            onChanged();
          }}
        />
        {info === null ? null : (
          <PlCard title="File">
            <PlFlex direction="vertical" spacing={3}>
              <PlSegmentedButton
                aria-label="Encryption"
                value={info.encrypted ? 'encrypted' : 'plain'}
                onValueChange={(value) => {
                  if (value !== null && (value === 'encrypted') !== info.encrypted) {
                    void handleEncryptionChange(value === 'encrypted');
                  }
                }}
                disabled={busy}
                fullWidth
                size="sm"
              >
                <PlSegment value="plain">Plain</PlSegment>
                <PlSegment value="encrypted">Encrypted</PlSegment>
              </PlSegmentedButton>
              <PlDataList orientation="vertical" size="sm">
                <PlDataListItem label="Path">
                  <span className="sample-path">{info.path}</span>
                </PlDataListItem>
                <PlDataListItem label="Size" value={formatBytes(info.bytes)} />
                <PlDataListItem
                  label="Format"
                  value={`version ${info.formatVersion}, pages of ${formatBytes(info.pageSize)}, ${info.encrypted ? 'encrypted' : 'not encrypted'}`}
                />
                <PlDataListItem
                  label="Engine"
                  value={`DaruDB ${info.engineVersion}, schema version ${info.schemaVersion ?? 'none'}`}
                />
              </PlDataList>
            </PlFlex>
          </PlCard>
        )}
      </PlFlex>
    </PlSidebar>
  );

  return (
    <PlPageLayout header={header} sidebar={sidebar} scroll="content" collapseBelow="none">
      <div className="sample-main">
        <CollectionView
          key={collection}
          collection={collection}
          version={version}
          onChanged={onChanged}
        />
      </div>
    </PlPageLayout>
  );
};
