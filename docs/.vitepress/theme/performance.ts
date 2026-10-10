/**
 * What the performance page says about the benchmark's stores and rows.
 *
 * This file, `performance.json` beside it, and the performance pages,
 * `docs/*\/performance.md`, are among the few places that name other database
 * products (`CLAUDE.md`). The numbers are in `performance.json`, which
 * `bench/merge.mjs` writes from the runs of `bench/run.mjs`; nothing here is
 * a measurement.
 */

import type { LanguageId } from '../languages';

export interface Localized {
  en: string;
  ko: string;
}

/** One cell: the median of the runs, in nanoseconds per operation. */
export interface Measured {
  median: number;
  min: number;
  max: number;
}

export interface Results {
  language: LanguageId;
  generated: string;
  runs: number;
  machine: { os: string; cpu: string; cores: number; memory?: number };
  stores: { id: string; version: string }[];
  /** A store with no way to do a row has `null` there. */
  rows: { id: string; count: number; agree: boolean; results: Record<string, Measured | null> }[];
}

export interface Data {
  languages: Partial<Record<LanguageId, Results>>;
}

/** The stores' names, as their projects write them. */
export const STORES: Record<string, string> = {
  daru: 'DaruDB',
  sqlite: 'SQLite',
  lmdb: 'LMDB',
  redb: 'redb',
  hive: 'Hive CE',
  realm: 'Realm'
};

/** The rows the page shows, in order; `left` is a check the page leaves out. */
export const ROWS: { id: string; label: Localized }[] = [
  {
    id: 'insert-sync',
    label: { en: 'Insert, one object per sync commit', ko: '삽입, 동기 커밋마다 객체 하나' }
  },
  {
    id: 'insert-deferred',
    label: { en: 'Insert, one object per deferred commit', ko: '삽입, 지연 커밋마다 객체 하나' }
  },
  {
    id: 'insert-bulk',
    label: {
      en: 'Insert 100,000 objects in one transaction',
      ko: '트랜잭션 하나로 객체 100,000개 삽입'
    }
  },
  {
    id: 'get-key',
    label: { en: 'Get by primary key, random order', ko: '기본 키로 읽기, 무작위 순서' }
  },
  {
    id: 'get-email',
    label: { en: 'Get by a unique index, random order', ko: '고유 인덱스로 읽기, 무작위 순서' }
  },
  {
    id: 'age-equal',
    label: { en: 'Query an indexed value, 1,250 objects', ko: '인덱스 값 쿼리, 객체 1,250개' }
  },
  {
    id: 'age-range',
    label: {
      en: 'Query an indexed range, sorted, first 20',
      ko: '인덱스 범위 쿼리, 정렬 후 20개'
    }
  },
  {
    id: 'count',
    label: { en: 'Count through an index', ko: '인덱스로 개수 세기' }
  },
  {
    id: 'city-scan',
    label: { en: 'Query without an index, 1,000 objects', ko: '인덱스 없는 쿼리, 객체 1,000개' }
  },
  {
    id: 'top-score',
    label: { en: 'Top 10 by a field without an index', ko: '인덱스 없는 필드로 상위 10개' }
  },
  {
    id: 'update',
    label: {
      en: 'Read and update 10,000 objects in one transaction',
      ko: '트랜잭션 하나로 객체 10,000개 읽고 고치기'
    }
  },
  {
    id: 'delete',
    label: {
      en: 'Delete 10,000 objects in one transaction',
      ko: '트랜잭션 하나로 객체 10,000개 지우기'
    }
  }
];

/** The few strings the table draws itself. */
export const TEXT = {
  en: {
    workload: 'Workload',
    measured:
      'Measured on {cpu} ({cores} cores), {os}, on {date}. Each time is the median of {runs} runs, per operation.',
    ratio:
      'Below each time, how many times as long as DaruDB the store takes: above 1, DaruDB is faster.',
    ratioTitle: '{store} takes {ratio} times as long as DaruDB',
    versions: 'Versions',
    missing: 'This language has no results yet.',
    none: '{store} has no way to do this'
  },
  ko: {
    workload: '작업',
    measured:
      '{cpu}({cores}코어), {os}에서 {date}에 쟀습니다. 시간은 {runs}번 실행한 값의 중앙값이며, 작업 한 번에 걸린 시간입니다.',
    ratio:
      '시간 아래의 수는 그 저장소가 DaruDB보다 몇 배 걸렸는지를 뜻합니다. 1보다 크면 DaruDB가 빠릅니다.',
    ratioTitle: '{store}는 DaruDB의 {ratio}배 걸립니다',
    versions: '버전',
    missing: '이 언어의 결과는 아직 없습니다.',
    none: '{store}에는 이 작업이 없습니다'
  }
};
