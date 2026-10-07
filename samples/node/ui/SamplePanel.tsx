/**
 * Inserting sample data: how many people, from which seed, and a progress
 * bar while the run commits its batches. The report keeps the time the
 * engine spent writing apart from the time spent making the objects.
 */
import { useState } from 'react';

import { PlButton } from 'plass-ui/button';
import { PlCard } from 'plass-ui/card';
import { PlDataList, PlDataListItem } from 'plass-ui/data-list';
import { PlFlex } from 'plass-ui/flex';
import { PlNumberField } from 'plass-ui/number-field';
import { PlProgressLinear } from 'plass-ui/progress-linear';
import { PlSegment, PlSegmentedButton } from 'plass-ui/segmented-button';
import { usePlToast } from 'plass-ui/toast';

import { planOf } from '../core/plan.ts';
import type { SeedProgress, SeedReport } from '../core/protocol.ts';

import { backend, describeError } from './backend.ts';
import { formatCount, formatDuration, formatRate } from './format.ts';

const SIZES = [1_000, 10_000, 100_000];

const STAGE_LABELS: Record<SeedProgress['stage'], string> = {
  pools: 'Preparing names and sentences',
  organizations: 'Inserting organizations',
  people: 'Inserting people',
  posts: 'Inserting posts'
};

interface SamplePanelProps {
  disabled: boolean;
  onStart: () => void;
  onFinish: () => void;
}

export const SamplePanel = ({ disabled, onStart, onFinish }: SamplePanelProps) => {
  const toast = usePlToast();
  const [people, setPeople] = useState(SIZES[0]);
  const [seed, setSeed] = useState<number | null>(1);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<SeedProgress | null>(null);
  const [report, setReport] = useState<SeedReport | null>(null);
  const plan = planOf(people);
  const total = plan.organizations + plan.people + plan.posts;

  /** How many objects of the run are in, by the stage that is going on. */
  const doneOf = (current: SeedProgress): number => {
    switch (current.stage) {
      case 'pools':
        return 0;
      case 'organizations':
        return current.done;
      case 'people':
        return plan.organizations + current.done;
      case 'posts':
        return plan.organizations + plan.people + current.done;
    }
  };

  const handleRun = async (): Promise<void> => {
    setRunning(true);
    setReport(null);
    setProgress(null);
    onStart();

    try {
      const finished = await backend.seed({ people, seed: seed ?? 0 }, setProgress);

      setReport(finished);
      toast.add({
        title: `Inserted ${formatCount(finished.organizations + finished.people + finished.posts)} objects`,
        color: 'success'
      });
    } catch (error) {
      const { code, message } = describeError(error);

      toast.add({ title: code, description: message, color: 'danger' });
    } finally {
      setRunning(false);
      setProgress(null);
      onFinish();
    }
  };

  return (
    <PlCard title="Sample data" subtitle="People in nine languages, with organizations and posts">
      <PlFlex direction="vertical" spacing={3}>
        <PlSegmentedButton
          aria-label="How many people"
          value={people}
          onValueChange={(value) => {
            if (typeof value === 'number') {
              setPeople(value);
            }
          }}
          fullWidth
          size="sm"
        >
          {SIZES.map((size) => (
            <PlSegment key={size} value={size}>
              {formatCount(size)}
            </PlSegment>
          ))}
        </PlSegmentedButton>
        <span className="sample-muted">
          {formatCount(plan.organizations)} organizations, {formatCount(plan.people)} people,{' '}
          {formatCount(plan.posts)} posts
        </span>
        <PlNumberField
          label="Seed"
          description="The same seed makes the same objects."
          value={seed}
          onValueChange={setSeed}
          min={0}
          max={4294967295}
          fullWidth
          size="sm"
        />
        <PlButton onClick={() => void handleRun()} loading={running} disabled={disabled}>
          Insert sample data
        </PlButton>
        {progress === null ? null : (
          <PlProgressLinear
            label={STAGE_LABELS[progress.stage]}
            value={doneOf(progress)}
            max={total}
            showValue
          />
        )}
        {report === null ? null : (
          <PlDataList orientation="horizontal" size="sm" labelWidth="7rem" data-testid="report">
            <PlDataListItem
              label="Objects"
              value={formatCount(report.organizations + report.people + report.posts)}
            />
            <PlDataListItem label="Writing" value={formatDuration(report.insertMs)} />
            <PlDataListItem
              label="Rate"
              value={formatRate(
                report.organizations + report.people + report.posts,
                report.insertMs
              )}
            />
            <PlDataListItem label="Generating" value={formatDuration(report.generateMs)} />
          </PlDataList>
        )}
      </PlFlex>
    </PlCard>
  );
};
