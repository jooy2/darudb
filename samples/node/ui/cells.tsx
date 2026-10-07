/** How the list draws a field's value in a cell. */
import type { ReactNode } from 'react';

import { PlChip } from 'plass-ui/chip';

import type { FieldInfo } from '../core/fields.ts';
import type { WireLocation, WireValue } from '../core/protocol.ts';

import { formatDate } from './format.ts';

/** The most tags a cell shows; the rest are counted. */
const TAGS_SHOWN = 3;

export const renderCell = (field: FieldInfo, value: WireValue | undefined): ReactNode => {
  if (value === null || value === undefined) {
    return <span className="sample-muted">null</span>;
  }

  switch (field.kind) {
    case 'color':
      return (
        <span
          className="sample-swatch"
          style={{ background: `#${value as string}` }}
          title={`#${value as string}`}
        />
      );
    case 'bool':
      return value ? 'yes' : 'no';
    case 'date':
      return formatDate(value as number);
    case 'float':
      return (value as number).toFixed(1);
    case 'tags': {
      const tags = value as string[];

      return (
        <span className="sample-tags">
          {tags.slice(0, TAGS_SHOWN).map((tag) => (
            <PlChip key={tag} size="xs">
              {tag}
            </PlChip>
          ))}
          {tags.length > TAGS_SHOWN ? (
            <span className="sample-muted">+{tags.length - TAGS_SHOWN}</span>
          ) : null}
        </span>
      );
    }
    case 'location': {
      const location = value as WireLocation;

      return [location.city, location.region, location.country]
        .filter((part) => part !== null)
        .join(', ');
    }
    default:
      return String(value);
  }
};
