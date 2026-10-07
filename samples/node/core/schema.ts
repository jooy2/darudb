/**
 * The sample's schema: three collections that between them use every kind of
 * field the engine has, so that the sample data exercises all of them.
 *
 * - `organizations` has a string primary key, `code`.
 * - `people` gets the automatic integer `id`, and holds a unique field, an
 *   optional unique field, an embedded object, a link, a list with an index,
 *   bytes, a float and a bool.
 * - `posts` links to `people`, and its optional list of tags has an index.
 *
 * `fields.ts` describes the same fields for the screens, and the Flutter
 * sample declares the same collections in `lib/src/model.dart`.
 */
import { collection, schema, t } from 'darudb';

export const sampleSchema = schema(1, {
  organizations: collection({
    code: t.string().primaryKey(),
    name: t.string().index(),
    kind: t.string().index(),
    industry: t.string().optional(),
    language: t.string(),
    founded: t.int().index()
  }),
  people: collection({
    name: t.string().index(),
    nickname: t.string().unique(),
    email: t.string().optional().unique(),
    age: t.int().index(),
    gender: t.string(),
    language: t.string().index(),
    location: t
      .object({
        country: t.string(),
        region: t.string().optional(),
        city: t.string().optional()
      })
      .optional(),
    organization: t.link('organizations').optional().index(),
    tags: t.list(t.string()).default([]).index(),
    active: t.bool().default(true),
    score: t.float().default(0),
    color: t.bytes(),
    joinedAt: t.int().index()
  }),
  posts: collection({
    author: t.link('people').index(),
    title: t.string(),
    body: t.string(),
    language: t.string().index(),
    tags: t.list(t.string()).optional().index(),
    likes: t.int().default(0).index(),
    pinned: t.bool().default(false),
    createdAt: t.int().index()
  })
});

export type SampleSchema = typeof sampleSchema;
