/**
 * How much a sample run makes from a number of people. Kept apart from
 * `sample.ts` so that the screens can say how far a run has come without
 * loading `randino` into the page.
 */

export const PEOPLE_PER_ORGANIZATION = 50;
export const POSTS_PER_PERSON = 3;

export interface Plan {
  organizations: number;
  people: number;
  posts: number;
}

/** How many of each a run of `people` people makes. */
export const planOf = (people: number): Plan => ({
  organizations: Math.ceil(people / PEOPLE_PER_ORGANIZATION),
  people,
  posts: people * POSTS_PER_PERSON
});
