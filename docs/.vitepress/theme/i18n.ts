/**
 * The few strings the theme draws itself, in each locale. Everything else on
 * the site is Markdown, which is written per locale already.
 */

const STRINGS = {
  en: {
    switchLabel: 'Language',
    switchList: 'The language you use DaruDB from',
    switchHint: 'Examples, the API and the types follow this choice on every page.',
    planned: 'Planned',
    packages: 'Packages',
    packagesHeading: 'Package registries',
    examplesIn: 'Examples in',
    startAction: 'Get started',
    demoQueries: 'Example queries',
    demoText: 'As text',
    demoCode: 'In code',
    demoCaption: 'The objects of the users collection, and the ones the query finds',
    demoFound: '{count} of {total} objects found'
  },
  ko: {
    switchLabel: '언어',
    switchList: 'DaruDB를 사용할 언어',
    switchHint: '모든 페이지의 예제와 API, 타입이 이 선택을 따릅니다.',
    planned: '예정',
    packages: '패키지',
    packagesHeading: '패키지 저장소',
    examplesIn: '예제 언어',
    startAction: '시작하기',
    demoQueries: '예제 쿼리',
    demoText: '문자열',
    demoCode: '코드',
    demoCaption: 'users 컬렉션의 객체와 그중 쿼리가 찾은 객체',
    demoFound: '객체 {total}개 중 {count}개를 찾았습니다'
  }
};

export type Locale = keyof typeof STRINGS;
export type StringKey = keyof (typeof STRINGS)['en'];

/** `ko-KR` → `ko`, and English for anything the site does not have. */
export function localeOf(lang: string): Locale {
  const short = lang.split('-')[0];

  return short in STRINGS ? (short as Locale) : 'en';
}

export function t(locale: Locale, key: StringKey): string {
  return STRINGS[locale][key];
}
