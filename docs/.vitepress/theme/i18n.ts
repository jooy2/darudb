/**
 * The few strings the theme draws itself, in each locale. Everything else on
 * the site is Markdown, which is written per locale already.
 */

const STRINGS = {
  en: {
    switchLabel: 'Language',
    switchList: 'The language you use DaruDB from',
    switchHint: 'Examples, the API and the types follow this choice on every page.',
    planned: 'Planned'
  },
  ko: {
    switchLabel: '언어',
    switchList: 'DaruDB를 사용할 언어',
    switchHint: '모든 페이지의 예제와 API, 타입이 이 선택을 따릅니다.',
    planned: '예정'
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
