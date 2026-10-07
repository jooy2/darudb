/**
 * The sample data: organizations, people and posts in nine languages, made
 * with `randino` from a seed.
 *
 * Generating every name and sentence one by one is slow (`randino` writes
 * about four thousand full sentences a second), so a run first fills pools
 * from the seed, a few hundred names, words, sentences and places for each
 * language, and each object is then a handful of draws from those pools.
 * Each object draws from a seed of its own, made from the run's seed and
 * the object's number, so object 1042 of seed 7 is the same object whichever
 * batch makes it and however many objects come before it in the run.
 *
 * The numbers continue from the objects already in the file, which keeps
 * the unique fields unique: an organization's code and a person's nickname
 * and email carry the number. The Flutter sample follows the same rules in
 * `lib/src/sample.dart`, though `randino` draws differently in each
 * language, so the two do not make the same names from the same seed.
 */
import {
  randAge,
  randCountry,
  randLocation,
  randName,
  randNickname,
  randOrganization,
  randSentence,
  randWord
} from 'randino';

import { Draw, seedOf } from './random.ts';

export const LANGUAGES = ['ko', 'en', 'ja', 'zh', 'de', 'es', 'it', 'ru', 'vi'] as const;

export type SampleLanguage = (typeof LANGUAGES)[number];

/** How often each of `LANGUAGES` comes up: Korean and English most. */
const LANGUAGE_WEIGHTS = [5, 5, 2, 2, 1, 1, 1, 1, 1];

/** The languages `randino` has places for, down to a city; the rest get a country. */
const PLACE_LANGUAGES: readonly SampleLanguage[] = ['ko', 'en'];

/** The languages whose sentences follow each other without a space. */
const UNSPACED_LANGUAGES: readonly SampleLanguage[] = ['ja', 'zh'];

/** What a pool or a kind of object draws from, mixed into its seed. */
const STREAMS = {
  names: 1,
  nicknames: 2,
  words: 3,
  titles: 4,
  sentences: 5,
  organizations: 6,
  places: 7,
  countries: 8,
  organization: 11,
  person: 12,
  post: 13
} as const;

const POOL_SIZES = {
  names: 400,
  nicknames: 1500,
  words: 120,
  titles: 150,
  sentences: 150,
  organizations: 200,
  places: 400,
  countries: 120
} as const;

/** The first of January 2020, and the six years after it, in milliseconds. */
const EPOCH_MS = Date.UTC(2020, 0, 1);
const SPAN_MS = Date.UTC(2026, 0, 1) - EPOCH_MS;

export interface OrganizationSample {
  code: string;
  name: string;
  kind: string;
  industry: string | null;
  language: string;
  founded: number;
}

export interface Place {
  country: string;
  region: string | null;
  city: string | null;
}

export interface PersonSample {
  name: string;
  nickname: string;
  email: string | null;
  age: number;
  gender: string;
  language: string;
  location: Place | null;
  organization: string | null;
  tags: string[];
  active: boolean;
  score: number;
  color: Uint8Array;
  joinedAt: number;
}

export interface PostSample {
  author: number;
  title: string;
  body: string;
  language: string;
  tags: string[] | null;
  likes: number;
  pinned: boolean;
  createdAt: number;
}

/** A person a post can be by: its key, and the language its posts are in. */
export interface Author {
  key: number;
  language: string;
}

interface LanguagePools {
  names: { native: string; gender: string }[];
  words: string[];
  titles: string[];
  sentences: string[];
  organizations: { name: string; kind: string; industry: string | null }[];
  places: Place[];
}

/** What every sample organization's code starts with. */
export const ORGANIZATION_PREFIX = 'ORG-';

/** An organization's code: its number, padded so that codes sort as numbers do. */
export const organizationCode = (number: number): string =>
  `${ORGANIZATION_PREFIX}${String(number).padStart(6, '0')}`;

export class SampleData {
  readonly seed: number;
  readonly #nicknames: string[];
  readonly #pools: Map<string, LanguagePools>;

  constructor(seed: number) {
    this.seed = seed;
    this.#nicknames = randNickname({
      language: 'en',
      count: POOL_SIZES.nicknames,
      unique: true,
      random: this.#stream(STREAMS.nicknames, 0)
    });
    this.#pools = new Map();

    for (const [index, language] of LANGUAGES.entries()) {
      this.#pools.set(language, this.#fill(language, index));
    }
  }

  /** Organization number `number`. */
  organization(number: number): OrganizationSample {
    const draw = new Draw(seedOf(this.seed, STREAMS.organization, number));
    const language = this.#language(draw);
    const made = draw.pick(this.#poolOf(language).organizations);

    return {
      code: organizationCode(number),
      name: made.name,
      kind: made.kind,
      industry: made.industry,
      language,
      founded: draw.between(1950, 2025)
    };
  }

  /** Person number `number`, who may belong to one of `organizations`, given by code. */
  person(number: number, organizations: readonly string[]): PersonSample {
    const draw = new Draw(seedOf(this.seed, STREAMS.person, number));
    const language = this.#language(draw);
    const pools = this.#poolOf(language);
    const name = draw.pick(pools.names);
    const nickname = `${draw.pick(this.#nicknames)}${number}`;

    return {
      name: name.native,
      nickname,
      email: draw.chance(0.8) ? `${nickname.toLowerCase()}@example.com` : null,
      age: randAge({ minAge: 14, maxAge: 90, random: draw.random })[0],
      gender: name.gender,
      language,
      location: draw.chance(0.95) ? draw.pick(pools.places) : null,
      organization: organizations.length > 0 && draw.chance(0.85) ? draw.pick(organizations) : null,
      tags: draw.picks(pools.words, draw.int(5)),
      active: draw.chance(0.9),
      score: Math.round(draw.random() * 1000) / 10,
      color: new Uint8Array([draw.int(256), draw.int(256), draw.int(256)]),
      joinedAt: EPOCH_MS + draw.int(SPAN_MS)
    };
  }

  /** Post number `number`, by one of `authors`. */
  post(number: number, authors: readonly Author[]): PostSample {
    const draw = new Draw(seedOf(this.seed, STREAMS.post, number));
    const author = draw.pick(authors);
    const pools = this.#poolOf(author.language);
    const separator = (UNSPACED_LANGUAGES as readonly string[]).includes(author.language)
      ? ''
      : ' ';

    return {
      author: author.key,
      title: draw.pick(pools.titles),
      body: draw.picks(pools.sentences, draw.between(1, 3)).join(separator),
      language: author.language,
      tags: draw.chance(0.6) ? draw.picks(pools.words, draw.between(1, 3)) : null,
      likes: Math.floor(draw.random() ** 3 * 500),
      pinned: draw.chance(0.02),
      createdAt: EPOCH_MS + draw.int(SPAN_MS)
    };
  }

  #stream(stream: number, language: number): () => number {
    return new Draw(seedOf(this.seed, stream, language)).random;
  }

  #language(draw: Draw): SampleLanguage {
    return draw.weighted(LANGUAGES, LANGUAGE_WEIGHTS);
  }

  #poolOf(language: string): LanguagePools {
    const pools = this.#pools.get(language);

    if (pools === undefined) {
      throw new Error(`no sample pools for the language ${language}`);
    }

    return pools;
  }

  #fill(language: SampleLanguage, index: number): LanguagePools {
    const names = randName({
      language,
      count: POOL_SIZES.names,
      output: 'detail',
      random: this.#stream(STREAMS.names, index)
    });
    const organizations = randOrganization({
      language,
      count: POOL_SIZES.organizations,
      output: 'detail',
      random: this.#stream(STREAMS.organizations, index)
    });

    return {
      names: names.map((name) => ({ native: name.native, gender: name.gender })),
      words: randWord({
        language,
        count: POOL_SIZES.words,
        unique: true,
        random: this.#stream(STREAMS.words, index)
      }),
      titles: randSentence({
        language,
        count: POOL_SIZES.titles,
        shape: 'simple',
        random: this.#stream(STREAMS.titles, index)
      }),
      sentences: randSentence({
        language,
        count: POOL_SIZES.sentences,
        random: this.#stream(STREAMS.sentences, index)
      }),
      organizations: organizations.map((made) => ({
        name: made.organization,
        kind: made.type,
        industry: made.industry
      })),
      places: this.#places(language, index)
    };
  }

  #places(language: SampleLanguage, index: number): Place[] {
    if (PLACE_LANGUAGES.includes(language)) {
      const places = randLocation({
        language: language as 'ko' | 'en',
        level: 'city',
        count: POOL_SIZES.places,
        output: 'detail',
        random: this.#stream(STREAMS.places, index)
      });

      return places.map((place) => ({
        country: place.country,
        region: place.region,
        city: place.city
      }));
    }

    const countries = randCountry({
      language,
      count: POOL_SIZES.countries,
      unique: true,
      random: this.#stream(STREAMS.countries, index)
    });

    return countries.map((country) => ({ country, region: null, city: null }));
  }
}
