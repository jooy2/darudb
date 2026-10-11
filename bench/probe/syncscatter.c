// Probe only: what fdatasync costs after a commit's pages are written
// scattered over a file, against the same pages written in one run, and
// against appending them to a log, on a GitHub-hosted runner.
//
//   cc -O2 syncscatter.c -o syncscatter && ./syncscatter DIR
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/uio.h>
#include <time.h>
#include <unistd.h>

#define PAGE 4096
#define FILE_PAGES 16384
#define ROUNDS 400

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e9 + t.tv_nsec;
}

static int open_filled(const char *path) {
  int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0644);
  static char page[PAGE];
  memset(page, 3, PAGE);
  for (int i = 0; i < FILE_PAGES; i++) pwrite(fd, page, PAGE, (off_t)i * PAGE);
  fsync(fd);
  return fd;
}

int main(int argc, char **argv) {
  char path[4096], log[4096];
  snprintf(path, sizeof path, "%s/scatter.bin", argv[1]);
  snprintf(log, sizeof log, "%s/log.bin", argv[1]);
  static char pages[16][PAGE];
  memset(pages, 7, sizeof pages);
  srand(7);

  for (int count = 2; count <= 12; count += 5) {
    int fd = open_filled(path);
    double start = now();
    for (int r = 0; r < ROUNDS; r++) {
      for (int i = 0; i < count; i++) pwrite(fd, pages[i], PAGE, (off_t)(rand() % FILE_PAGES) * PAGE);
      pwrite(fd, pages[0], 64, 0);
      fdatasync(fd);
    }
    double scattered = (now() - start) / ROUNDS;

    start = now();
    for (int r = 0; r < ROUNDS; r++) {
      struct iovec io[16];
      for (int i = 0; i < count; i++) { io[i].iov_base = pages[i]; io[i].iov_len = PAGE; }
      pwritev(fd, io, count, (off_t)(rand() % (FILE_PAGES - count)) * PAGE);
      pwrite(fd, pages[0], 64, 0);
      fdatasync(fd);
    }
    double run = (now() - start) / ROUNDS;

    // Scattered again, each page's writeback started as soon as it is
    // written, so that the barrier waits for writes already under way.
    start = now();
    for (int r = 0; r < ROUNDS; r++) {
      for (int i = 0; i < count; i++) {
        off_t at = (off_t)(rand() % FILE_PAGES) * PAGE;
        pwrite(fd, pages[i], PAGE, at);
        sync_file_range(fd, at, PAGE, SYNC_FILE_RANGE_WRITE);
      }
      pwrite(fd, pages[0], 64, 0);
      fdatasync(fd);
    }
    double started = (now() - start) / ROUNDS;

    // Scattered, with one call that starts the writeback of the whole file
    // before the barrier.
    start = now();
    for (int r = 0; r < ROUNDS; r++) {
      for (int i = 0; i < count; i++) pwrite(fd, pages[i], PAGE, (off_t)(rand() % FILE_PAGES) * PAGE);
      sync_file_range(fd, 0, 0, SYNC_FILE_RANGE_WRITE);
      pwrite(fd, pages[0], 64, 0);
      fdatasync(fd);
    }
    double started_once = (now() - start) / ROUNDS;
    close(fd);

    int lf = open(log, O_RDWR | O_CREAT | O_TRUNC, 0644);
    off_t end = 0;
    start = now();
    for (int r = 0; r < ROUNDS; r++) {
      struct iovec io[16];
      for (int i = 0; i < count; i++) { io[i].iov_base = pages[i]; io[i].iov_len = PAGE; }
      pwritev(lf, io, count, end);
      end += (off_t)count * PAGE;
      fdatasync(lf);
    }
    double appended = (now() - start) / ROUNDS;
    close(lf);

    printf("%2d pages: scattered %.0f us, one run %.0f us, scattered with writeback started per page %.0f us, "
           "scattered with one call to start it %.0f us, appended to a log %.0f us\n",
           count, scattered / 1000, run / 1000, started / 1000, started_once / 1000, appended / 1000);
  }
  return 0;
}
