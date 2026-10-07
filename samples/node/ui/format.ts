/** How the screens write numbers, sizes, dates and durations. */

const COUNT = new Intl.NumberFormat('en-US');

export const formatCount = (value: number): string => COUNT.format(value);

export const formatBytes = (bytes: number): string => {
  const units = ['B', 'KiB', 'MiB', 'GiB'];
  let value = bytes;
  let unit = 0;

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return `${unit === 0 ? value : value.toFixed(1)} ${units[unit]}`;
};

/** A time in milliseconds since 1970 as its date, `2024-05-17`. */
export const formatDate = (ms: number): string => new Date(ms).toISOString().slice(0, 10);

/** A duration in milliseconds: `840 ms`, or `3.2 s` past a second. */
export const formatDuration = (ms: number): string =>
  ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`;

/** Objects per second, from a count and the milliseconds it took. */
export const formatRate = (count: number, ms: number): string =>
  ms <= 0 ? '' : `${formatCount(Math.round((count / ms) * 1000))} objects/s`;
