// Probe only: what each system call a one-object commit makes costs on this
// machine, in nanoseconds a call.
//
//   syscost FILE
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static double now(void) {
  struct timespec t;

  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e9 + t.tv_nsec;
}

static void report(const char *what, double started, long count) {
  printf("%-44s %9.0f ns\n", what, (now() - started) / count);
}

int main(int argc, char **argv) {
  const char *path = argc > 1 ? argv[1] : "syscost.tmp";
  int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0644);
  char page[4096];
  struct stat st;
  struct flock lock = {.l_whence = SEEK_SET, .l_start = 1LL << 62, .l_len = 1};
  double started;
  long n;

  if (fd < 0) {
    perror(path);
    return 1;
  }
  memset(page, 7, sizeof page);

  for (n = 0; n < 64; n++) pwrite(fd, page, sizeof page, n * 4096);

  started = now();
  for (n = 0; n < 200000; n++) getppid();
  report("getppid", started, 200000);

  started = now();
  for (n = 0; n < 200000; n++) pwrite(fd, page, sizeof page, (n % 64) * 4096);
  report("pwrite 4 KiB over a page written before", started, 200000);

  started = now();
  for (n = 0; n < 200000; n++) pwrite(fd, page, 1, 4096 + (n % 64));
  report("pwrite 1 byte", started, 200000);

  ftruncate(fd, 0);
  started = now();
  for (n = 0; n < 50000; n++) pwrite(fd, page, sizeof page, n * 4096);
  report("pwrite 4 KiB growing the file", started, 50000);

  started = now();
  for (n = 0; n < 200000; n++) fstat(fd, &st);
  report("fstat", started, 200000);

  started = now();
  for (n = 0; n < 100000; n++) {
    lock.l_type = F_WRLCK;
    fcntl(fd, F_SETLK, &lock);
    lock.l_type = F_UNLCK;
    fcntl(fd, F_SETLK, &lock);
  }
  report("fcntl F_SETLK, lock and unlock", started, 100000);

  started = now();
  for (n = 0; n < 200000; n++) {
    lock.l_type = F_WRLCK;
    fcntl(fd, F_GETLK, &lock);
  }
  report("fcntl F_GETLK", started, 200000);

  // Flush what the loops above wrote, so that the next two time one page.
  fsync(fd);

  started = now();
  for (n = 0; n < 200; n++) {
    pwrite(fd, page, sizeof page, (n % 64) * 4096);
    fdatasync(fd);
  }
  report("pwrite 4 KiB and fdatasync", started, 200);

  started = now();
  for (n = 0; n < 200; n++) {
    pwrite(fd, page, sizeof page, (n % 64) * 4096);
    fsync(fd);
  }
  report("pwrite 4 KiB and fsync", started, 200);

  close(fd);
  unlink(path);
  return 0;
}
