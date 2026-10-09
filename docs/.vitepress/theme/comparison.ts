/**
 * What the comparison page says about each database, one cell per feature.
 *
 * This file and the comparison pages, `docs/*\/compare.md`, are the only
 * places outside the migration guides that name other database products
 * (`CLAUDE.md`). Every value was checked against the project's own
 * documentation, source or registry pages in October 2026; the date on the
 * page says so, and a change here should be checked the same way.
 *
 * A cell is `yes`, `partial` or `no`, with a short note where the mark alone
 * would mislead, or `text` for a row that holds a value rather than a yes or
 * no, such as the licence. Notes are written in both locales.
 */

import type { LanguageId } from '../languages';

export type Level = 'yes' | 'partial' | 'no' | 'text';

export interface Localized {
  en: string;
  ko: string;
}

export interface Cell {
  level: Level;
  note?: Localized;
}

export interface Product {
  id: string;
  name: string;
  /** The project's own site, which the column's heading links to. */
  site?: string;
  /** DaruDB's own column, drawn apart from the others. */
  self?: boolean;
}

export interface Row {
  id: string;
  label: Localized;
  /** For a row about one programming language: which one. */
  language?: LanguageId;
  cells: Record<string, Cell>;
}

export interface Section {
  id: string;
  label: Localized;
  rows: Row[];
}

export const PRODUCTS: Product[] = [
  { id: 'darudb', name: 'DaruDB', self: true },
  { id: 'sqlite', name: 'SQLite', site: 'https://sqlite.org/' },
  { id: 'realm', name: 'Realm', site: 'https://github.com/realm' },
  { id: 'objectbox', name: 'ObjectBox', site: 'https://objectbox.io/' },
  { id: 'isar', name: 'Isar', site: 'https://isar.dev/' },
  { id: 'hive', name: 'Hive', site: 'https://pub.dev/packages/hive_ce' },
  { id: 'lmdb', name: 'LMDB', site: 'https://www.symas.com/mdb' },
  { id: 'rocksdb', name: 'RocksDB', site: 'https://rocksdb.org/' },
  { id: 'redb', name: 'redb', site: 'https://www.redb.org/' }
];

const yes = (en?: string, ko?: string): Cell => cell('yes', en, ko);
const partly = (en: string, ko?: string): Cell => cell('partial', en, ko);
const no = (en?: string, ko?: string): Cell => cell('no', en, ko);
const value = (en: string, ko: string = en): Cell => cell('text', en, ko);

function cell(level: Level, en?: string, ko?: string): Cell {
  return en === undefined ? { level } : { level, note: { en, ko: ko ?? en } };
}

const row = (
  id: string,
  en: string,
  ko: string,
  cells: Record<string, Cell>,
  language?: LanguageId
): Row => ({ id, label: { en, ko }, cells, ...(language ? { language } : {}) });

/** The same cell for every product, for a platform row most of them share. */
const each = (cells: Partial<Record<string, Cell>>, fallback: Cell): Record<string, Cell> =>
  Object.fromEntries(PRODUCTS.map((product) => [product.id, cells[product.id] ?? fallback]));

const NOT_IN_ISAR_3 = no('Not in Isar 3', 'Isar 3에는 없음');
const KEPT_BY_APP = no('Kept by the application', '애플리케이션이 직접 관리');

export const SECTIONS: Section[] = [
  {
    id: 'overview',
    label: { en: 'Overview', ko: '개요' },
    rows: [
      row('model', 'Data model', '데이터 모델', {
        darudb: value('Objects in collections', '컬렉션의 객체'),
        sqlite: value('Relational tables, SQL', '관계형 테이블, SQL'),
        realm: value('Objects', '객체'),
        objectbox: value('Objects, with vector search', '객체, 벡터 검색 포함'),
        isar: value('Objects in collections', '컬렉션의 객체'),
        hive: value('Key-value boxes', '키-값 박스'),
        lmdb: value('Ordered key-value', '정렬된 키-값'),
        rocksdb: value('Ordered key-value, LSM tree', '정렬된 키-값, LSM 트리'),
        redb: value('Typed key-value tables', '타입이 있는 키-값 테이블')
      }),
      row('license', 'License', '라이선스', {
        darudb: value('MIT'),
        sqlite: value('Public domain', '퍼블릭 도메인'),
        realm: value('Apache-2.0'),
        objectbox: value(
          'Apache-2.0 bindings, closed-source core',
          'Apache-2.0 바인딩, 비공개 코어'
        ),
        isar: value('Apache-2.0'),
        hive: value('Apache-2.0; BSD-3-Clause for hive_ce', 'Apache-2.0, hive_ce는 BSD-3-Clause'),
        lmdb: value('OpenLDAP Public License'),
        rocksdb: value('Apache-2.0 or GPLv2', 'Apache-2.0 또는 GPLv2'),
        redb: value('MIT or Apache-2.0', 'MIT 또는 Apache-2.0')
      }),
      row('maintained', 'Maintained', '유지 관리', {
        darudb: yes(),
        sqlite: yes(),
        realm: no(
          'Deprecated by its vendor in 2024; community branches release rarely since',
          '2024년 공급사가 지원 중단, 이후 커뮤니티 브랜치의 릴리스는 드묾'
        ),
        objectbox: yes(),
        isar: partly(
          'The original is dormant since 2023; isar_community carries version 3',
          '원본은 2023년 이후 멈춤, isar_community가 버전 3을 이어 감'
        ),
        hive: partly(
          'The original is dormant since 2022; hive_ce carries it',
          '원본은 2022년 이후 멈춤, hive_ce가 이어 감'
        ),
        lmdb: yes('Version 1.0 in 2026', '2026년 버전 1.0'),
        rocksdb: yes(),
        redb: yes()
      })
    ]
  },
  {
    id: 'languages',
    label: { en: 'Packages for each language', ko: '언어별 패키지' },
    rows: [
      row(
        'rust',
        'Rust',
        'Rust',
        {
          darudb: yes('darudb, darudb-derive'),
          sqlite: yes('rusqlite (third party)', 'rusqlite (서드파티)'),
          realm: no(),
          objectbox: no(),
          isar: no('Its Rust core has no public API', 'Rust 코어에 공개 API 없음'),
          hive: no(),
          lmdb: yes('heed (third party)', 'heed (서드파티)'),
          rocksdb: yes('rocksdb (third party)', 'rocksdb (서드파티)'),
          redb: yes('redb')
        },
        'rust'
      ),
      row(
        'node',
        'Node.js',
        'Node.js',
        {
          darudb: yes('darudb'),
          sqlite: yes(
            'node:sqlite, built into Node.js; better-sqlite3',
            'Node.js 내장 node:sqlite, better-sqlite3'
          ),
          realm: partly('realm, no longer developed by its vendor', 'realm, 공급사 개발 중단'),
          objectbox: no(),
          isar: no(),
          hive: no(),
          lmdb: yes('lmdb (third party)', 'lmdb (서드파티)'),
          rocksdb: partly(
            '@harperfast/rocksdb-js; the older binding is discontinued',
            '@harperfast/rocksdb-js, 예전 바인딩은 중단됨'
          ),
          redb: no()
        },
        'node'
      ),
      row(
        'dart',
        'Dart and Flutter',
        'Dart와 Flutter',
        {
          darudb: yes('darudb, darudb_generator'),
          sqlite: yes(
            'sqflite, sqlite3, drift (third party)',
            'sqflite, sqlite3, drift (서드파티)'
          ),
          realm: partly('realm, no longer developed by its vendor', 'realm, 공급사 개발 중단'),
          objectbox: yes('objectbox'),
          isar: yes('isar_community, or the original isar', 'isar_community, 또는 원본 isar'),
          hive: yes('hive_ce, or the original hive', 'hive_ce, 또는 원본 hive'),
          lmdb: partly('Small third-party wrappers', '소규모 서드파티 래퍼'),
          rocksdb: no(),
          redb: no()
        },
        'dart'
      ),
      row(
        'python',
        'Python',
        'Python',
        {
          darudb: yes('darudb'),
          sqlite: yes('sqlite3 in the standard library', '표준 라이브러리 sqlite3'),
          realm: no(),
          objectbox: partly(
            'objectbox, an alpha release from 2024',
            'objectbox, 2024년의 알파 버전'
          ),
          isar: no(),
          hive: no(),
          lmdb: yes('lmdb (third party)', 'lmdb (서드파티)'),
          rocksdb: yes('rocksdict (third party)', 'rocksdict (서드파티)'),
          redb: no(
            'An early binding without tables, from 2022',
            '테이블이 없는 2022년의 초기 바인딩'
          )
        },
        'python'
      )
    ]
  },
  {
    id: 'storage',
    label: { en: 'Storage and durability', ko: '저장과 내구성' },
    rows: [
      row('acid', 'ACID transactions', 'ACID 트랜잭션', {
        darudb: yes(),
        sqlite: yes(),
        realm: yes(),
        objectbox: yes(),
        isar: yes(),
        hive: no('No transactions', '트랜잭션 없음'),
        lmdb: yes(),
        rocksdb: partly(
          'Atomic batches; transactions are an option',
          '원자적 배치, 트랜잭션은 선택'
        ),
        redb: yes()
      }),
      row('crash', 'The file survives a crash or a power cut', '크래시와 정전에도 파일이 온전함', {
        darudb: yes('A commit is durable when it returns', '커밋이 반환될 때 이미 디스크에 기록됨'),
        sqlite: yes('With the default journal and sync settings', '기본 저널·동기화 설정에서'),
        realm: yes(),
        objectbox: yes(),
        isar: yes(),
        hive: partly(
          'Opening drops a damaged tail; writes are never flushed by themselves',
          '열 때 손상된 끝부분을 버림, 쓰기를 스스로 플러시하지 않음'
        ),
        lmdb: yes('Copy-on-write, no recovery step', 'copy-on-write, 복구 단계 없음'),
        rocksdb: yes(
          'The log restores it; unsynced writes are lost',
          '로그로 복원, 동기화 전 쓰기는 사라짐'
        ),
        redb: yes()
      }),
      row('relaxed', 'Commits that leave the flush for later', '디스크 플러시를 미루는 커밋', {
        darudb: yes(
          'Deferred commits; a process crash loses none',
          '지연 커밋, 프로세스가 죽어도 잃지 않음'
        ),
        sqlite: yes('synchronous=NORMAL in WAL mode', 'WAL 모드의 synchronous=NORMAL'),
        realm: no(),
        objectbox: no(),
        isar: yes('relaxedDurability, on by default', 'relaxedDurability, 기본값으로 켜짐'),
        hive: value(
          'Every write: flush() is what reaches the disk',
          '모든 쓰기, 디스크 기록은 flush() 때'
        ),
        lmdb: yes('MDB_NOMETASYNC, MDB_NOSYNC'),
        rocksdb: yes('The default; syncing is the option', '기본 동작, 동기화가 선택'),
        redb: yes('Durability::None')
      }),
      row('processes', 'Several processes on one file', '여러 프로세스가 한 파일 사용', {
        darudb: yes(
          'Through the system’s file locks alone, with no shared memory',
          '운영체제 파일 잠금만 사용, 공유 메모리 없음'
        ),
        sqlite: yes(
          'One writer at a time; WAL mode shares memory through a mapped file',
          '쓰기는 한 번에 하나, WAL 모드는 매핑한 파일로 메모리 공유'
        ),
        realm: partly(
          'Through mutexes in a shared-memory lock file, for one architecture and version',
          '공유 메모리 잠금 파일의 뮤텍스로, 아키텍처와 버전이 같을 때만'
        ),
        objectbox: partly(
          'One process writes; others only read',
          '쓰는 프로세스는 하나, 나머지는 읽기만'
        ),
        isar: no(
          'One process; isolates share an instance',
          '한 프로세스만, isolate끼리는 인스턴스 공유'
        ),
        hive: no(
          'One process per box, held by a lock file',
          '박스마다 한 프로세스, 잠금 파일로 강제'
        ),
        lmdb: yes(
          'Through mutexes in a shared-memory lock file',
          '공유 메모리 잠금 파일의 뮤텍스로'
        ),
        rocksdb: partly(
          'One process writes; others open secondary instances',
          '한 프로세스만 쓰기, 나머지는 보조 인스턴스'
        ),
        redb: partly(
          'Read-only sharing, or an experimental feature',
          '읽기 전용 공유, 또는 실험적 기능'
        )
      }),
      row('readers', 'Readers never wait for a writer', '읽기가 쓰기를 기다리지 않음', {
        darudb: yes(),
        sqlite: partly('In WAL mode', 'WAL 모드에서'),
        realm: yes(),
        objectbox: yes(),
        isar: yes(),
        hive: partly('Reads come from memory, with no snapshot', '메모리에서 읽음, 스냅샷 없음'),
        lmdb: yes(),
        rocksdb: yes(),
        redb: yes()
      })
    ]
  },
  {
    id: 'encryption',
    label: { en: 'Encryption', ko: '암호화' },
    rows: [
      row('encryption', 'Built-in encryption', '내장 암호화', {
        darudb: yes(
          'Every page, with XAES-256-GCM or XChaCha20-Poly1305',
          '모든 페이지를 XAES-256-GCM이나 XChaCha20-Poly1305로'
        ),
        sqlite: partly(
          'Through an extension: SQLCipher, SQLite3 Multiple Ciphers, or the paid SEE',
          '확장으로: SQLCipher, SQLite3 Multiple Ciphers, 유료 SEE'
        ),
        realm: yes('AES-256-CBC on every page', '페이지마다 AES-256-CBC'),
        objectbox: no(
          'Offered on request, not in the libraries',
          '요청 시 제공, 라이브러리에는 없음'
        ),
        isar: NOT_IN_ISAR_3,
        hive: partly('AES-256-CBC on values; keys stay plain', '값만 AES-256-CBC, 키는 평문'),
        lmdb: partly(
          'Version 1.0 calls a cipher the application supplies',
          '버전 1.0이 애플리케이션이 넘긴 암호를 호출'
        ),
        rocksdb: partly(
          'A framework for file encryption, with no production cipher',
          '파일 암호화 틀만, 실사용 암호는 없음'
        ),
        redb: no()
      }),
      row('aead', 'Tampering is detected', '변조를 감지함', {
        darudb: yes('Pages and commit records', '페이지와 커밋 기록 모두'),
        sqlite: partly(
          'SQLCipher adds an HMAC to every page',
          'SQLCipher가 페이지마다 HMAC을 붙임'
        ),
        realm: yes('An HMAC-SHA224 on every page', '페이지마다 HMAC-SHA224'),
        objectbox: no(),
        isar: NOT_IN_ISAR_3,
        hive: no(),
        lmdb: partly('When the cipher given is an AEAD', '넘긴 암호가 AEAD일 때'),
        rocksdb: no('CTR mode, unauthenticated', '인증 없는 CTR 모드'),
        redb: no()
      }),
      row('kdf', 'A key made from a password', '비밀번호로 키를 만듦', {
        darudb: yes('Argon2id'),
        sqlite: partly('SQLCipher: PBKDF2-HMAC-SHA512'),
        realm: no('A raw 64-byte key', '64바이트 원시 키'),
        objectbox: no(),
        isar: NOT_IN_ISAR_3,
        hive: no('A raw 32-byte key', '32바이트 원시 키'),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('rekey', 'Change the key in place', '키를 제자리에서 교체', {
        darudb: yes(
          'Rewraps the data key; a backup changes the data key itself',
          '데이터 키를 다시 감쌈, 데이터 키 자체는 백업으로 교체'
        ),
        sqlite: partly(
          'SQLCipher’s PRAGMA rekey encrypts every page again',
          'SQLCipher의 PRAGMA rekey가 모든 페이지를 다시 암호화'
        ),
        realm: no('Write an encrypted copy instead', '암호화한 사본을 새로 써야 함'),
        objectbox: no(),
        isar: NOT_IN_ISAR_3,
        hive: no(),
        lmdb: no('Copy into a new environment', '새 환경으로 복사'),
        rocksdb: no(),
        redb: no()
      })
    ]
  },
  {
    id: 'data',
    label: { en: 'Schema and queries', ko: '스키마와 쿼리' },
    rows: [
      row('schema', 'Typed schema', '타입이 있는 스키마', {
        darudb: yes('Stored in the file', '파일에 저장'),
        sqlite: partly(
          'Types belong to values; STRICT tables enforce them',
          '타입은 값에 붙음, STRICT 테이블은 열 타입 강제'
        ),
        realm: yes(),
        objectbox: yes(),
        isar: yes('Generated from annotated classes', '어노테이션한 클래스에서 생성'),
        hive: partly(
          'Generated adapters the store does not enforce',
          '저장소가 강제하지 않는 생성 어댑터'
        ),
        lmdb: no('Bytes', '바이트'),
        rocksdb: no('Bytes', '바이트'),
        redb: yes('Key and value types per table', '테이블마다 키·값 타입')
      }),
      row('migrations', 'Versioned migrations', '버전별 마이그레이션', {
        darudb: yes(
          'Additive changes by themselves, a function for each version',
          '추가 변경은 자동, 버전마다 함수'
        ),
        sqlite: partly(
          'Not built in; drift and sqflite add version steps',
          '내장되지 않음, drift와 sqflite가 버전 단계 제공'
        ),
        realm: yes(),
        objectbox: partly(
          'Additive changes by themselves; no version steps',
          '추가 변경은 자동, 버전 단계 없음'
        ),
        isar: partly(
          'Additive changes by themselves; no version steps',
          '추가 변경은 자동, 버전 단계 없음'
        ),
        hive: no('Field numbers allow adding and removing fields', '필드 번호로 필드 추가·삭제만'),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('indexes', 'Secondary indexes', '보조 인덱스', {
        darudb: yes(),
        sqlite: yes(),
        realm: yes(),
        objectbox: yes(),
        isar: yes(),
        hive: no(),
        lmdb: KEPT_BY_APP,
        rocksdb: partly('Experimental, through transactions', '트랜잭션을 거치는 실험적 기능'),
        redb: KEPT_BY_APP
      }),
      row('unique', 'Unique indexes', '유니크 인덱스', {
        darudb: yes(),
        sqlite: yes(),
        realm: partly('Primary keys only', '기본 키만'),
        objectbox: yes(),
        isar: yes(),
        hive: no(),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('composite', 'Composite indexes', '복합 인덱스', {
        darudb: no('Planned', '계획됨'),
        sqlite: yes(),
        realm: no(),
        objectbox: no(),
        isar: yes('Up to three fields', '최대 세 필드'),
        hive: no(),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('querylang', 'Text query language', '문자열 쿼리 언어', {
        darudb: yes(),
        sqlite: yes('SQL'),
        realm: yes('Realm Query Language'),
        objectbox: no(),
        isar: no(),
        hive: no(),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('builder', 'Typed query builder', '타입이 있는 쿼리 빌더', {
        darudb: yes('In every language', '모든 언어에서'),
        sqlite: partly(
          'drift for Dart; other bindings take SQL',
          'Dart의 drift, 다른 바인딩은 SQL'
        ),
        realm: partly('Not in JavaScript or Dart', 'JavaScript와 Dart에는 없음'),
        objectbox: yes(),
        isar: yes(),
        hive: no(),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('sort', 'Sort, limit and offset', '정렬, limit, offset', {
        darudb: yes(),
        sqlite: yes(),
        realm: yes('Offset by slicing the results', 'offset은 결과를 잘라서'),
        objectbox: yes('No sorting in Python yet', 'Python에는 아직 정렬 없음'),
        isar: yes(),
        hive: no('Done in Dart', 'Dart 코드에서 직접'),
        lmdb: partly('Ranges in key order', '키 순서의 범위 탐색'),
        rocksdb: partly('Ranges in key order', '키 순서의 범위 탐색'),
        redb: partly('Ranges in key order', '키 순서의 범위 탐색')
      }),
      row('links', 'Links between objects, or joins', '객체 간 링크나 조인', {
        darudb: partly(
          'Links, followed in queries; no backlinks or joins',
          '쿼리에서 따라가는 링크, 역링크와 조인은 없음'
        ),
        sqlite: yes('Joins and foreign keys', '조인과 외래 키'),
        realm: yes('Links and backlinks', '링크와 역링크'),
        objectbox: yes('Relations and backlinks', '관계와 역링크'),
        isar: yes('Links and backlinks', '링크와 역링크'),
        hive: partly('HiveList, deprecated in hive_ce', 'HiveList, hive_ce에서는 비권장'),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('embedded', 'Embedded objects and lists', '내장 객체와 리스트', {
        darudb: partly('No list of embedded objects yet', '내장 객체의 리스트는 아직 없음'),
        sqlite: partly('As JSON in a column', '열에 담은 JSON으로'),
        realm: yes(),
        objectbox: partly(
          'Lists of scalars; no embedded objects',
          '스칼라 리스트, 내장 객체는 없음'
        ),
        isar: yes(),
        hive: yes(),
        lmdb: no('Values are bytes', '값은 바이트'),
        rocksdb: no('Values are bytes', '값은 바이트'),
        redb: partly('Tuples, vectors and custom value types', '튜플, 벡터, 사용자 값 타입')
      }),
      row('fts', 'Full-text search', '전문 검색', {
        darudb: no(),
        sqlite: yes('FTS5'),
        realm: partly('Whole words, Latin scripts only', '단어 단위, 라틴 문자만'),
        objectbox: no(),
        isar: partly('Word lists in an index', '인덱스에 단어 목록을 담아서'),
        hive: no(),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('aggregates', 'Aggregations beyond counting', '개수 외의 집계', {
        darudb: no('Counting only', '개수만'),
        sqlite: yes(),
        realm: yes(),
        objectbox: yes(),
        isar: yes(),
        hive: no(),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      })
    ]
  },
  {
    id: 'api',
    label: { en: 'API', ko: 'API' },
    rows: [
      row('syncapi', 'Synchronous API', '동기 API', {
        darudb: yes('Every language', '모든 언어'),
        sqlite: yes(),
        realm: yes(),
        objectbox: yes(),
        isar: yes(),
        hive: partly('Reads of a normal box', '일반 박스의 읽기'),
        lmdb: yes(),
        rocksdb: yes(),
        redb: yes()
      }),
      row('asyncapi', 'Asynchronous API', '비동기 API', {
        darudb: partly('Node.js, Dart and Python; not Rust', 'Node.js, Dart, Python, Rust는 없음'),
        sqlite: partly(
          'sqflite and drift for Dart; the others are synchronous',
          'Dart의 sqflite와 drift, 나머지는 동기'
        ),
        realm: partly('Opening, and writes in Dart', '열기, 그리고 Dart의 쓰기'),
        objectbox: partly('In Dart; not in Python', 'Dart에서, Python에는 없음'),
        isar: yes(),
        hive: yes(),
        lmdb: partly('Writes in lmdb-js', 'lmdb-js의 쓰기'),
        rocksdb: partly('In rocksdb-js', 'rocksdb-js에서'),
        redb: no()
      }),
      row('watch', 'Change notifications', '변경 알림', {
        darudb: no('Left out on purpose', '의도적으로 제외'),
        sqlite: partly(
          'An update hook per connection; drift’s query streams',
          '연결별 update hook, drift의 쿼리 스트림'
        ),
        realm: yes('Live objects and queries', '라이브 객체와 쿼리'),
        objectbox: yes('Query observers', '쿼리 옵저버'),
        isar: yes('Watchers on objects, collections and queries', '객체·컬렉션·쿼리 감시'),
        hive: yes('box.watch'),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      }),
      row('remote', 'Sync with a server', '서버와 동기화', {
        darudb: no('A local file only', '로컬 파일 전용'),
        sqlite: no(),
        realm: no('The vendor’s sync service has ended', '공급사의 동기화 서비스 종료'),
        objectbox: partly('A paid add-on', '유료 추가 기능'),
        isar: no(),
        hive: no(),
        lmdb: no(),
        rocksdb: no(),
        redb: no()
      })
    ]
  },
  {
    id: 'tools',
    label: { en: 'Tools', ko: '도구' },
    rows: [
      row('check', 'Integrity check', '무결성 검사', {
        darudb: yes('While others write', '다른 쓰기가 진행되는 중에도'),
        sqlite: yes('PRAGMA integrity_check'),
        realm: no(),
        objectbox: yes('When opening', '열 때'),
        isar: no('An experimental verify(), for tests', '테스트용 실험적 verify()만'),
        hive: partly('A checksum check when a box opens', '박스를 열 때 체크섬 검사'),
        lmdb: partly('Page checksums in 1.0; no checker', '1.0의 페이지 체크섬, 검사 도구는 없음'),
        rocksdb: yes('VerifyChecksum'),
        redb: yes('check_integrity')
      }),
      row('salvage', 'Rescue a damaged file', '손상된 파일 살리기', {
        darudb: yes(
          'A new file from the pages that pass their checks',
          '검사를 통과한 페이지로 새 파일을 만듦'
        ),
        sqlite: yes('The recovery extension, .recover', '복구 확장, .recover'),
        realm: no(),
        objectbox: partly('Opening the previous commit', '이전 커밋으로 열기'),
        isar: no(),
        hive: partly('Drops what follows the first damaged entry', '처음 손상된 항목 뒤를 버림'),
        lmdb: no('Opening the previous snapshot at most', '이전 스냅샷을 여는 정도'),
        rocksdb: yes('RepairDB, which may lose data', 'RepairDB, 데이터가 빠질 수 있음'),
        redb: partly('Repair on open, after a crash', '크래시 뒤 열 때 복구')
      }),
      row('backup', 'Backup while in use', '사용 중 백업', {
        darudb: yes(),
        sqlite: yes('Backup API, VACUUM INTO', '백업 API, VACUUM INTO'),
        realm: yes('A copy of an open file', '열린 파일의 사본'),
        objectbox: no('In server builds only', '서버 빌드에만'),
        isar: yes('copyToFile'),
        hive: no(),
        lmdb: yes('mdb_copy; incremental in 1.0', 'mdb_copy, 1.0의 증분 백업'),
        rocksdb: yes('BackupEngine, Checkpoint'),
        redb: no()
      }),
      row('compact', 'Compaction', '파일 크기 줄이기', {
        darudb: yes('In place, while in use', '사용 중에 제자리에서'),
        sqlite: yes('VACUUM, which needs room for a copy', 'VACUUM, 사본만큼 여유 공간 필요'),
        realm: yes('When opening, or on request', '열 때나 요청할 때'),
        objectbox: no(),
        isar: partly('When opening', '열 때'),
        hive: yes('Automatic, by a strategy', '전략에 따라 자동'),
        lmdb: partly('Into a compacted copy', '압축한 사본으로'),
        rocksdb: yes('In the background', '백그라운드에서'),
        redb: yes('compact()')
      })
    ]
  },
  {
    id: 'platforms',
    label: { en: 'Platforms', ko: '플랫폼' },
    rows: [
      row('windows', 'Windows', 'Windows', each({}, yes())),
      row('macos', 'macOS', 'macOS', each({}, yes())),
      row('linux', 'Linux', 'Linux', each({}, yes())),
      row(
        'ios',
        'iOS',
        'iOS',
        each(
          {
            darudb: yes('Rust and Dart', 'Rust와 Dart'),
            lmdb: partly('No official statement', '공식 지원 언급 없음'),
            redb: partly('Pure Rust, not tested in its CI', '순수 Rust, CI에서 시험하지 않음')
          },
          yes()
        )
      ),
      row(
        'android',
        'Android',
        'Android',
        each(
          {
            darudb: yes('Rust, Node.js and Dart', 'Rust, Node.js, Dart'),
            rocksdb: partly(
              'A build target, not listed as supported',
              '빌드 대상이지만 지원 목록에 없음'
            ),
            redb: partly('Pure Rust, not tested in its CI', '순수 Rust, CI에서 시험하지 않음')
          },
          yes()
        )
      ),
      row(
        'browser',
        'Browser',
        '브라우저',
        each(
          {
            darudb: no('Out of scope', '대상 아님'),
            sqlite: yes('Official WebAssembly build', '공식 WebAssembly 빌드'),
            hive: yes('On IndexedDB', 'IndexedDB 위에서'),
            isar: NOT_IN_ISAR_3,
            redb: no('Memory only in WebAssembly', 'WebAssembly에서는 메모리에만')
          },
          no()
        )
      )
    ]
  }
];

/** The languages a product has a package for, read off the language rows. */
export function languagesOf(id: string): LanguageId[] {
  const rows = SECTIONS.find((section) => section.id === 'languages')?.rows ?? [];

  return rows
    .filter((item) => item.language && item.cells[id]?.level !== 'no')
    .map((item) => item.language as LanguageId);
}
