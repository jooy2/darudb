/**
 * The dialog that adds an object or changes one. Each field gets the input
 * its kind needs, and saving sends the whole object: `insert` for a new one,
 * `update` with every field but the key for one that exists. What the engine
 * refuses, such as a nickname another person holds, shows in the dialog with
 * the engine's code, and the dialog stays open.
 */
import { useId, useState } from 'react';
import type { FormEvent, ReactNode } from 'react';

import { PlAlert } from 'plass-ui/alert';
import { PlButton } from 'plass-ui/button';
import { PlColorPicker } from 'plass-ui/color-picker';
import { PlFieldset } from 'plass-ui/fieldset';
import { PlModal } from 'plass-ui/modal';
import { PlNumberField } from 'plass-ui/number-field';
import { PlSwitch } from 'plass-ui/switch';
import { PlTextField } from 'plass-ui/text-field';

import { COLLECTIONS } from '../core/fields.ts';
import type { CollectionName, FieldInfo } from '../core/fields.ts';
import type { Key, WireObject } from '../core/protocol.ts';

import { backend, describeError } from './backend.ts';
import { draftOf, formFields, wireOf } from './draft.ts';
import type { Draft, DraftValue, LocationDraft } from './draft.ts';

interface ObjectFormProps {
  collection: CollectionName;
  /** The object to change, or `null` for a new one. */
  object: WireObject | null;
  onClose: () => void;
  onSaved: (key: Key, inserted: boolean) => void;
}

const DESCRIPTIONS: Partial<Record<string, string>> = {
  tags: 'Separate tags with commas.',
  organization: 'The code of an organization, such as ORG-000001.',
  author: 'The id of a person.'
};

export const ObjectForm = ({ collection, object, onClose, onSaved }: ObjectFormProps) => {
  const formId = useId();
  const info = COLLECTIONS[collection];
  const [draft, setDraft] = useState<Draft>(() => draftOf(collection, object));
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [failure, setFailure] = useState<{ code: string; message: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const key = object === null ? null : (object[info.key] as Key);

  const set = (name: string, value: DraftValue): void => {
    setDraft((current) => ({ ...current, [name]: value }));
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();

    const { object: wire, errors: found } = wireOf(collection, draft);

    setErrors(found);
    setFailure(null);

    if (Object.keys(found).length > 0) {
      return;
    }

    setSaving(true);

    try {
      if (key === null) {
        onSaved(await backend.call('insert', { collection, object: wire }), true);
      } else {
        const changes = { ...wire };

        delete changes[info.key];
        await backend.call('update', { collection, key, changes });
        onSaved(key, false);
      }
    } catch (error) {
      setFailure(describeError(error));
    } finally {
      setSaving(false);
    }
  };

  const labelOf = (field: FieldInfo): string =>
    field.optional ? `${field.name} (optional)` : field.name;

  const inputOf = (field: FieldInfo): ReactNode => {
    const value = draft[field.name];
    const error = errors[field.name];
    const readOnly = key !== null && field.name === info.key;

    switch (field.kind) {
      case 'int':
      case 'float':
        return (
          <PlNumberField
            key={field.name}
            name={field.name}
            label={labelOf(field)}
            value={value as number | null}
            onValueChange={(next) => set(field.name, next)}
            step={field.kind === 'int' ? 1 : 0.1}
            error={error}
            fullWidth
          />
        );
      case 'bool':
        return (
          <PlSwitch
            key={field.name}
            name={field.name}
            label={field.name}
            checked={value as boolean}
            onCheckedChange={(checked: boolean) => set(field.name, checked)}
          />
        );
      case 'color':
        return (
          <PlColorPicker
            key={field.name}
            name={field.name}
            label={field.name}
            value={value as string}
            onValueChange={(next) => set(field.name, next)}
            swatches={false}
          />
        );
      case 'location': {
        const location = value as LocationDraft;
        const setPart = (part: keyof LocationDraft, text: string): void =>
          set(field.name, { ...location, [part]: text });

        return (
          <PlFieldset
            key={field.name}
            className="sample-wide"
            legend={labelOf(field)}
            description={error}
          >
            <PlTextField
              label="country"
              value={location.country}
              onChange={(event) => setPart('country', event.target.value)}
              fullWidth
            />
            <PlTextField
              label="region (optional)"
              value={location.region}
              onChange={(event) => setPart('region', event.target.value)}
              fullWidth
            />
            <PlTextField
              label="city (optional)"
              value={location.city}
              onChange={(event) => setPart('city', event.target.value)}
              fullWidth
            />
          </PlFieldset>
        );
      }
      default:
        return (
          <PlTextField
            key={field.name}
            name={field.name}
            label={labelOf(field)}
            type={field.kind === 'date' ? 'date' : 'text'}
            value={value as string}
            onChange={(event) => set(field.name, event.target.value)}
            multiline={field.name === 'body'}
            className={field.name === 'body' ? 'sample-wide' : undefined}
            description={DESCRIPTIONS[field.name]}
            error={error}
            readOnly={readOnly}
            fullWidth
          />
        );
    }
  };

  return (
    <PlModal
      open
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
      title={key === null ? `New object in ${collection}` : `Edit ${collection} ${key}`}
      size="lg"
      actions={
        <>
          <PlButton variant="ghost" type="button" onClick={onClose}>
            Cancel
          </PlButton>
          <PlButton type="submit" form={formId} loading={saving}>
            Save
          </PlButton>
        </>
      }
    >
      <form id={formId} className="sample-form" onSubmit={(event) => void handleSubmit(event)}>
        {failure === null ? null : (
          <PlAlert className="sample-wide" color="danger" title={failure.code}>
            {failure.message}
          </PlAlert>
        )}
        {formFields(collection).map(inputOf)}
      </form>
    </PlModal>
  );
};
