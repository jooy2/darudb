/**
 * An object while its form is open: each field as its input holds it, text
 * for most, a number, a switch's state or a colour. `draftOf` makes one from
 * an object or from nothing, and `wireOf` turns it back into the object to
 * send, with a message for each field that cannot be one.
 *
 * Only what a form cannot leave to the engine is checked here, such as a
 * required number that is empty. Everything else, a nickname another person
 * holds for one, is the engine's to refuse.
 */
import { COLLECTIONS } from '../core/fields.ts';
import type { CollectionName, FieldInfo } from '../core/fields.ts';
import type { WireLocation, WireObject, WireValue } from '../core/protocol.ts';

import { formatDate } from './format.ts';

export interface LocationDraft {
  country: string;
  region: string;
  city: string;
}

export type DraftValue = string | number | null | boolean | LocationDraft;

export type Draft = Record<string, DraftValue>;

/** The fields a form shows: every field, the key too unless the engine assigns it. */
export const formFields = (collection: CollectionName): readonly FieldInfo[] =>
  COLLECTIONS[collection].fields;

const today = (): string => formatDate(Date.now());

const emptyValue = (field: FieldInfo): DraftValue => {
  switch (field.kind) {
    case 'int':
    case 'float':
      return field.optional ? null : 0;
    case 'bool':
      return field.name === 'active';
    case 'date':
      return today();
    case 'color':
      return '#3d7be0';
    case 'location':
      return { country: '', region: '', city: '' };
    default:
      return '';
  }
};

const draftValue = (field: FieldInfo, value: WireValue | undefined): DraftValue => {
  if (value === undefined || value === null) {
    return emptyValue(field);
  }

  switch (field.kind) {
    case 'date':
      return formatDate(value as number);
    case 'color':
      return `#${value as string}`;
    case 'tags':
      return (value as string[]).join(', ');
    case 'location': {
      const location = value as WireLocation;

      return {
        country: location.country,
        region: location.region ?? '',
        city: location.city ?? ''
      };
    }
    case 'link':
      return String(value);
    default:
      return value as DraftValue;
  }
};

/** A draft of `object`, or of a new object of `collection`. */
export const draftOf = (collection: CollectionName, object: WireObject | null): Draft => {
  const draft: Draft = {};

  for (const field of formFields(collection)) {
    draft[field.name] = draftValue(field, object?.[field.name]);
  }

  return draft;
};

const textOr = (field: FieldInfo, text: string): string | null =>
  text.trim() === '' && field.optional ? null : text.trim();

const wireValue = (field: FieldInfo, value: DraftValue): WireValue => {
  switch (field.kind) {
    case 'int':
    case 'float': {
      if (value === null && !field.optional) {
        throw new Error('needs a number');
      }

      if (field.kind === 'int' && value !== null && !Number.isInteger(value)) {
        throw new Error('needs a whole number');
      }

      return value as number | null;
    }
    case 'bool':
      return value as boolean;
    case 'date': {
      const ms = Date.parse(`${value as string}T00:00:00Z`);

      if (Number.isNaN(ms)) {
        throw new Error('needs a date');
      }

      return ms;
    }
    case 'color':
      return (value as string).replace('#', '').toLowerCase();
    case 'tags': {
      const tags = (value as string)
        .split(',')
        .map((tag) => tag.trim())
        .filter((tag) => tag.length > 0);

      return tags.length === 0 && field.optional ? null : tags;
    }
    case 'location': {
      const location = value as LocationDraft;

      if (location.country.trim() === '') {
        if (!field.optional) {
          throw new Error('needs a country');
        }

        return null;
      }

      return {
        country: location.country.trim(),
        region: location.region.trim() === '' ? null : location.region.trim(),
        city: location.city.trim() === '' ? null : location.city.trim()
      };
    }
    case 'link': {
      const text = textOr(field, value as string);

      if (text === null || field.target !== 'people') {
        return text;
      }

      const key = Number(text);

      if (!Number.isSafeInteger(key) || key < 1) {
        throw new Error('needs the id of a person');
      }

      return key;
    }
    default:
      return textOr(field, value as string);
  }
};

/** The object a draft describes, or the message of each field that is wrong. */
export const wireOf = (
  collection: CollectionName,
  draft: Draft
): { object: WireObject; errors: Record<string, string> } => {
  const object: WireObject = {};
  const errors: Record<string, string> = {};

  for (const field of formFields(collection)) {
    try {
      object[field.name] = wireValue(field, draft[field.name]);
    } catch (error) {
      errors[field.name] = `${field.name} ${(error as Error).message}`;
    }
  }

  return { object, errors };
};
