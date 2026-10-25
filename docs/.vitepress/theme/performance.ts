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
  /** `complex` for the complex suite; results from before it name none. */
  suite?: 'simple' | 'complex';
  generated: string;
  runs: number;
  machine: { os: string; cpu: string; cores: number; memory?: number };
  stores: { id: string; version: string }[];
  /** A store with no way to do a row has `null` there. */
  rows: { id: string; count: number; agree: boolean; results: Record<string, Measured | null> }[];
}

export interface Data {
  languages: Partial<Record<LanguageId, Results>>;
  /** The complex suite's results, which only the stores with a query language run. */
  complex?: Partial<Record<LanguageId, Results>>;
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
    id: 'city-scan-first',
    label: {
      en: 'Query without an index, 1,000 objects, first run',
      ko: '인덱스 없는 쿼리, 객체 1,000개, 처음 실행'
    }
  },
  {
    id: 'top-score',
    label: { en: 'Top 10 by a field without an index', ko: '인덱스 없는 필드로 상위 10개' }
  },
  {
    id: 'top-score-first',
    label: {
      en: 'Top 10 by a field without an index, first run',
      ko: '인덱스 없는 필드로 상위 10개, 처음 실행'
    }
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

/** The rows of the complex suite, in order; `left-people` and `left-posts` are checks. */
export const COMPLEX_ROWS: { id: string; label: Localized }[] = [
  {
    id: 'load',
    label: {
      en: 'Load 401,000 objects, 10,000 to a transaction',
      ko: '객체 401,000개 적재, 트랜잭션마다 10,000개'
    }
  },
  {
    id: 'person-detail',
    label: {
      en: 'A person, their organization and their 5 newest posts',
      ko: '사람 한 명과 그 조직, 최신 게시글 5개'
    }
  },
  {
    id: 'people-filter',
    label: {
      en: 'Three conditions on two indexes, sorted, first 50',
      ko: '인덱스 둘에 걸친 조건 셋, 정렬 후 50개'
    }
  },
  {
    id: 'people-or',
    label: { en: 'Count with OR across two indexes', ko: '인덱스 둘에 걸친 OR로 개수 세기' }
  },
  {
    id: 'people-prefix',
    label: { en: 'Names that start with a text', ko: '어떤 글로 시작하는 이름' }
  },
  {
    id: 'people-page',
    label: {
      en: 'A page of 50, deep in an indexed order',
      ko: '인덱스 순서 깊숙한 곳의 50개 한 페이지'
    }
  },
  {
    id: 'posts-author-country',
    label: {
      en: 'Through a link: posts by country of author, sorted, first 20',
      ko: '링크 하나 건너: 작성자 국가로 게시글, 정렬 후 20개'
    }
  },
  {
    id: 'posts-two-links',
    label: {
      en: 'Through two links: count by kind of organization',
      ko: '링크 둘 건너: 조직 종류로 개수 세기'
    }
  },
  {
    id: 'posts-tag',
    label: {
      en: 'A tag in a list, sorted by another index, first 20',
      ko: '리스트 안의 태그, 다른 인덱스로 정렬 후 20개'
    }
  },
  {
    id: 'posts-text',
    label: { en: 'Count posts whose text contains a phrase', ko: '본문에 구절이 든 게시글 개수' }
  },
  {
    id: 'posts-count',
    label: { en: 'Count with conditions on two indexes', ko: '인덱스 둘에 걸친 조건으로 개수 세기' }
  },
  {
    id: 'update-query',
    label: {
      en: 'Update the people a query finds, in one transaction',
      ko: '쿼리로 찾은 사람 고치기, 트랜잭션 하나로'
    }
  },
  {
    id: 'delete-cascade',
    label: {
      en: 'Delete 1,000 people with their posts, in one transaction',
      ko: '사람 1,000명과 그 게시글 지우기, 트랜잭션 하나로'
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
