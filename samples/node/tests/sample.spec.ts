/**
 * The sample from a user's side: an empty database, sample data inserted in
 * bulk, the objects listed, filtered, sorted and paged by the engine, one
 * object added, changed, refused and deleted, the file checked and
 * compacted, the data still there after the app starts again, and the file
 * reset. The steps run in order on one database, in both projects.
 */
import type { Page } from '@playwright/test';

import { expect, test } from './fixtures.ts';

test.describe.configure({ mode: 'serial' });

const SEED = 7;

const countOf = (page: Page, collection: string) => page.getByTestId(`count-${collection}`);

const openCollection = async (page: Page, collection: string): Promise<void> => {
  await page.getByRole('button', { name: new RegExp(`^${collection} [\\d,]+ objects$`) }).click();
  await expect(page.getByRole('heading', { name: new RegExp(`^${collection}`) })).toBeVisible();
};

const applyFilter = async (page: Page, filter: string): Promise<void> => {
  await page.getByLabel('Filter', { exact: true }).fill(filter);
  await page.getByRole('button', { name: 'Apply' }).click();
};

/** The texts of one column of the table, by its header. */
const columnOf = async (page: Page, header: string): Promise<string[]> => {
  const headers = await page.locator('table thead th').allTextContents();
  const index = headers.findIndex((text) => text.trim() === header);

  expect(index).toBeGreaterThanOrEqual(0);

  return page.locator(`table tbody tr td:nth-child(${index + 1})`).allTextContents();
};

/** The question a confirmation asks, the one dialog open at the time. */
const confirmDialog = (page: Page) => page.getByRole('dialog');

const insertSampleData = async (page: Page): Promise<void> => {
  await page.getByLabel('Seed', { exact: true }).fill(String(SEED));
  await page.getByRole('button', { name: 'Insert sample data' }).click();
  await expect(page.getByText('Inserted 4,020 objects')).toBeVisible();
};

test('starts with an empty database', async ({ sample }) => {
  const { page } = sample;

  await expect(countOf(page, 'organizations')).toHaveText('0');
  await expect(countOf(page, 'people')).toHaveText('0');
  await expect(countOf(page, 'posts')).toHaveText('0');
  await expect(page.getByText('No objects')).toBeVisible();
});

test('inserts sample data in bulk', async ({ sample }) => {
  const { page } = sample;

  await page.getByRole('radio', { name: '1,000' }).click();
  await insertSampleData(page);
  await expect(countOf(page, 'organizations')).toHaveText('20');
  await expect(countOf(page, 'people')).toHaveText('1,000');
  await expect(countOf(page, 'posts')).toHaveText('3,000');
  await expect(page.getByTestId('report')).toContainText('4,020');
  await expect(page.getByTestId('total')).toHaveText('1,000');
});

test('continues the numbers on a second run with the same seed', async ({ sample }) => {
  const { page } = sample;

  // The same seed again would repeat every nickname and code, and the unique
  // index would refuse them, if a run did not continue from the file's own
  // numbers.
  await insertSampleData(page);
  await expect(countOf(page, 'organizations')).toHaveText('40');
  await expect(countOf(page, 'people')).toHaveText('2,000');
  await expect(countOf(page, 'posts')).toHaveText('6,000');
});

test('filters, sorts and pages through people', async ({ sample }) => {
  const { page } = sample;

  await openCollection(page, 'people');
  await applyFilter(page, 'age >= 65');
  // The filter finds some people, and only people of 65 or older.
  await expect(page.getByTestId('total')).not.toHaveText('2,000');
  await expect(page.getByTestId('total')).not.toHaveText('0');
  await expect
    .poll(async () => (await columnOf(page, 'age')).every((age) => Number(age) >= 65))
    .toBe(true);

  // Twice: ascending, then descending.
  await page.getByRole('button', { name: 'age', exact: true }).click();
  await page.getByRole('button', { name: 'age', exact: true }).click();
  await expect(page.locator('th', { hasText: /^age/ })).toHaveAttribute('aria-sort', 'descending');
  await expect
    .poll(async () => {
      const ages = (await columnOf(page, 'age')).map(Number);

      return ages.length > 1 && ages.every((age, i) => i === 0 || ages[i - 1] >= age);
    })
    .toBe(true);

  const firstPage = await columnOf(page, 'id');

  await page.getByRole('button', { name: 'Next page' }).click();
  await expect.poll(async () => (await columnOf(page, 'id'))[0]).not.toBe(firstPage[0]);
});

test('finds posts through their author', async ({ sample }) => {
  const { page } = sample;

  await openCollection(page, 'posts');
  await applyFilter(page, 'author.language == "ko"');
  await expect(page.getByTestId('total')).not.toHaveText('6,000');
  await expect(page.getByTestId('total')).not.toHaveText('0');
  await expect
    .poll(async () => (await columnOf(page, 'language')).every((language) => language === 'ko'))
    .toBe(true);
});

test('shows the engine refusing a filter that does not parse', async ({ sample }) => {
  const { page } = sample;

  await openCollection(page, 'people');
  await applyFilter(page, 'age >>');
  await expect(page.getByRole('alert').filter({ hasText: 'INVALID_QUERY' })).toBeVisible();
  await page.getByRole('button', { name: 'Clear' }).click();
  await expect(page.getByTestId('total')).toHaveText('2,000');
});

test('adds, changes and deletes a person', async ({ sample }) => {
  const { page } = sample;
  const dialog = page.getByRole('dialog');

  await openCollection(page, 'people');
  await page.getByRole('button', { name: 'Add object' }).click();
  await dialog.getByLabel('name', { exact: true }).fill('End To End');
  await dialog.getByLabel('nickname', { exact: true }).fill('e2e-tester');
  await dialog.getByLabel('age', { exact: true }).fill('33');
  await dialog.getByLabel('gender', { exact: true }).fill('female');
  await dialog.getByLabel('language', { exact: true }).fill('en');
  await dialog.getByLabel('tags', { exact: true }).fill('sample, test');
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText(/^Inserted people \d+$/)).toBeVisible();
  await expect(countOf(page, 'people')).toHaveText('2,001');

  await applyFilter(page, 'nickname == "e2e-tester"');
  await expect(page.getByTestId('total')).toHaveText('1');
  await page
    .getByRole('row', { name: /e2e-tester/ })
    .getByRole('button', { name: 'Edit' })
    .click();
  await dialog.getByLabel('age', { exact: true }).fill('34');
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText(/^Updated people \d+$/)).toBeVisible();
  await expect.poll(() => columnOf(page, 'age')).toEqual(['34']);

  // A second person with the same nickname: the unique index refuses it.
  await page.getByRole('button', { name: 'Add object' }).click();
  await dialog.getByLabel('name', { exact: true }).fill('Someone Else');
  await dialog.getByLabel('nickname', { exact: true }).fill('e2e-tester');
  await dialog.getByLabel('gender', { exact: true }).fill('male');
  await dialog.getByLabel('language', { exact: true }).fill('en');
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(dialog.getByRole('alert').filter({ hasText: 'DUPLICATE_KEY' })).toBeVisible();
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(countOf(page, 'people')).toHaveText('2,001');

  await page
    .getByRole('row', { name: /e2e-tester/ })
    .getByRole('button', { name: 'Delete' })
    .click();
  await confirmDialog(page).getByRole('button', { name: 'Delete' }).click();
  await expect(page.getByTestId('total')).toHaveText('0');
  await expect(countOf(page, 'people')).toHaveText('2,000');
});

test('adds an organization under a key of its own', async ({ sample }) => {
  const { page } = sample;
  const dialog = page.getByRole('dialog');

  await openCollection(page, 'organizations');
  await page.getByRole('button', { name: 'Add object' }).click();
  await dialog.getByLabel('code', { exact: true }).fill('E2E-0001');
  await dialog.getByLabel('name', { exact: true }).fill('End To End Ltd.');
  await dialog.getByLabel('kind', { exact: true }).fill('company');
  await dialog.getByLabel('language', { exact: true }).fill('en');
  await dialog.getByLabel('founded', { exact: true }).fill('2024');
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Inserted organizations E2E-0001')).toBeVisible();
  await expect(countOf(page, 'organizations')).toHaveText('41');
});

test('checks and compacts the file', async ({ sample }) => {
  const { page } = sample;

  await page.getByRole('button', { name: 'Check' }).click();
  await expect(page.getByText('The file is intact')).toBeVisible();
  await page.getByRole('button', { name: 'Compact' }).click();
  await expect(page.getByText('Compacted')).toBeVisible();
});

test('keeps what it committed when the app starts again', async ({ sample }) => {
  const page = await sample.restart();

  await expect(countOf(page, 'organizations')).toHaveText('41');
  await expect(countOf(page, 'people')).toHaveText('2,000');
  await expect(countOf(page, 'posts')).toHaveText('6,000');
});

test('resets the database', async ({ sample }) => {
  const { page } = sample;

  await page.getByRole('button', { name: 'Reset' }).click();
  await confirmDialog(page).getByRole('button', { name: 'Reset' }).click();
  await expect(page.getByText('The database is empty')).toBeVisible();
  await expect(countOf(page, 'organizations')).toHaveText('0');
  await expect(countOf(page, 'people')).toHaveText('0');
  await expect(countOf(page, 'posts')).toHaveText('0');
});
