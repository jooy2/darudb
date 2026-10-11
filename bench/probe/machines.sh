#!/usr/bin/env bash
# Probe only: what one runner is, what single system calls cost on it, and
# where DaruDB's, SQLite's and LMDB's deferred commits spend their time there.
#
#   machines.sh OUT
set -euo pipefail

out=$1
bin=bench/rust/target/release/darudb-bench

mkdir -p "$out"
{
  lscpu | grep -E 'Model name|^CPU\(s\)'
  uname -r
  grep -H . /sys/devices/system/cpu/vulnerabilities/{spectre_v2,spec_rstack_overflow,retbleed} || true
  findmnt -no SOURCE,FSTYPE,OPTIONS -T /tmp
  for queue in /sys/block/*/queue; do
    echo "$queue stable_writes=$(cat "$queue/stable_writes" 2> /dev/null) rotational=$(cat "$queue/rotational")"
  done
} > "$out/machine.txt"

gcc -O2 -o syscost bench/probe/syscost.c
./syscost "$(mktemp -d)/file" > "$out/syscost.txt"

node bench/probe/ab.mjs 3 "daru=$bin:daru" "sqlite=$bin:sqlite" "lmdb=$bin:lmdb" > "$out/ab.txt"

for store in daru sqlite lmdb; do
  dir=$(mktemp -d)
  BENCH_SKIP=insert-sync BENCH_FOCUS=insert-deferred perf stat -e task-clock,context-switches,page-faults \
    -o "$out/stat-$store.txt" "$bin" --child "$store" --dir "$dir" > /dev/null
  rm -rf "$dir"
  dir=$(mktemp -d)
  BENCH_SKIP=insert-sync BENCH_FOCUS=insert-deferred perf record -q -e cpu-clock -F 4999 \
    -o "perf-$store.data" "$bin" --child "$store" --dir "$dir" > /dev/null
  rm -rf "$dir"
  {
    perf report -i "perf-$store.data" --no-children --sort dso --stdio 2> /dev/null | grep -v '^#' | grep -v '^$' | head -6
    perf report -i "perf-$store.data" --no-children --sort symbol --stdio --percent-limit 1 2> /dev/null | grep -v '^#' | grep -v '^$' | head -40
  } > "$out/profile-$store.txt"
done
