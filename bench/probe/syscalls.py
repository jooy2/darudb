"""Probe only: what each pass traced with `strace -f -T` did in each row.

    python3 syscalls.py TRACE...

Rows are told apart by the markers the probe harness stats. For each row,
each system call is counted per operation of the row, with its mean time,
keyed by the file it was made on; writes that end past every earlier write to
their file are counted as growing it.
"""

import os
import re
import sys
from collections import defaultdict

OPS = {
    'insert-sync': 500,
    'insert-deferred': 10_000,
    'insert-bulk': 1,
    'get-key': 100_000,
    'get-email': 20_000,
    'age-equal': 200,
    'age-range': 5_000,
    'count': 200,
    'city-scan': 10,
    'top-score': 10,
    'update': 1,
    'delete': 1,
}
LINE = re.compile(r'^(\d+)\s+(.*)$')
CALL = re.compile(r'^(\w+)\((.*)$')
RESUMED = re.compile(r'^<\.\.\. (\w+) resumed>(.*)$')
RESULT = re.compile(r'\)\s+=\s+(-?\d+|0x[0-9a-f]+|\?)[^<]*<([\d.]+)>\s*$')
MARK = re.compile(r'"/bench-marker/([\w-]+)"')
PATH = re.compile(r'"([^"]+)"')
WRITES = {'pwrite64', 'pwritev', 'pwritev2', 'write'}


def report(path):
    rows = defaultdict(lambda: defaultdict(lambda: [0, 0.0]))
    growing = defaultdict(int)
    samples = defaultdict(list)
    files = {}
    ends = {}
    pending = {}
    row = 'open'

    with open(path, errors='replace') as trace:
        for raw in trace:
            match = LINE.match(raw.rstrip('\n'))

            if not match:
                continue

            pid, text = match.groups()

            if text.endswith('<unfinished ...>'):
                call = CALL.match(text)

                if call:
                    name, args = call.groups()
                    pending[pid] = (name, args[: -len('<unfinished ...>')].rstrip())
                continue

            resumed = RESUMED.match(text)

            if resumed:
                name, rest = resumed.groups()
                args = pending.pop(pid, (name, ''))[1] + rest
            else:
                call = CALL.match(text)

                if not call:
                    continue
                name, args = call.groups()

            marker = MARK.search(args)

            if marker and name in ('statx', 'newfstatat', 'stat'):
                row = marker.group(1)
                continue

            result = RESULT.search(args)

            if not result:
                continue

            value, seconds = result.group(1), float(result.group(2))
            leading = re.match(r'\s*(\d+)\b', args)
            first = leading.group(1) if leading else ''
            target = ''

            if name == 'openat':
                found = PATH.search(args)

                if found and value.isdigit():
                    files[value] = os.path.basename(found.group(1))
                    ends.pop(value, None)
                target = os.path.basename(found.group(1)) if found else ''
            elif name == 'close':
                files.pop(first, None)
                ends.pop(first, None)
                target = ''
            elif first.isdigit():
                target = files.get(first, f'fd {first}')

            key = f'{name} {target}'.strip()

            if name == 'fcntl':
                command = args.split(',')[1].strip() if ',' in args else ''
                key = f'{key} {command}'

            if name in WRITES and first.isdigit() and value.lstrip('-').isdigit():
                numbers = re.findall(r',\s*(\d+)\)', args[: result.start() + 1])
                offset = int(numbers[-1]) if numbers and name != 'write' else None

                if offset is not None:
                    end = offset + int(value)

                    if first in ends and end > ends[first]:
                        growing[row, target] += 1
                    ends[first] = max(ends.get(first, 0), end)

            cell = rows[row][key]
            cell[0] += 1
            cell[1] += seconds

            if row in ('insert-sync', 'insert-deferred') and len(samples[row]) < 400:
                samples[row].append(f'{name}({args[:110]}')

    print(f'\n######## {os.path.basename(path)}')

    for row, calls in rows.items():
        ops = OPS.get(row, 1)
        print(f'\n-- {row} (per operation, {ops} operations)')

        for key, (count, seconds) in sorted(calls.items(), key=lambda item: -item[1][1]):
            print(f'  {key:<46} {count / ops:9.3f} calls  {seconds / count * 1e6:9.1f} us each  {seconds * 1e3:9.1f} ms in all')

        for (grown_row, target), count in growing.items():
            if grown_row == row:
                print(f'  writes that grew {target}: {count / ops:.3f} per operation')

    for row, lines in samples.items():
        middle = lines[len(lines) // 2 :][:40]
        print(f'\n-- {row}: 40 calls from the middle of the first 400')

        for line in middle:
            print(f'  {line}')


for path in sys.argv[1:]:
    report(path)
