// Probe only: what ten scattered 4 KiB writes into the page cache cost as
// ten pwrite calls, as one pwritev each for runs, and as one io_uring
// submission, which is what a commit's writes would become.
//
//   cc -O2 uring.c -luring -o uring && ./uring FILE
#define _GNU_SOURCE
#include <fcntl.h>
#include <liburing.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define PAGE 4096
#define WRITES 10
#define ROUNDS 20000

static double now(void) {
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return t.tv_sec * 1e9 + t.tv_nsec;
}

int main(int argc, char **argv) {
  int fd = open(argv[1], O_RDWR | O_CREAT | O_TRUNC, 0644);
  static char buf[WRITES][PAGE] __attribute__((aligned(4096)));
  off_t at[WRITES];

  memset(buf, 7, sizeof buf);
  if (ftruncate(fd, (off_t)PAGE * 8192) != 0) return 1;
  for (int i = 0; i < 8192; i++) pwrite(fd, buf[0], PAGE, (off_t)i * PAGE);

  for (int pass = 0; pass < 3; pass++) {
    double start = now();
    for (int r = 0; r < ROUNDS; r++) {
      for (int i = 0; i < WRITES; i++) at[i] = (off_t)((r * 131 + i * 811) % 8192) * PAGE;
      for (int i = 0; i < WRITES; i++)
        if (pwrite(fd, buf[i], PAGE, at[i]) != PAGE) return 2;
    }
    double plain = (now() - start) / ROUNDS;

    struct io_uring ring;
    if (io_uring_queue_init(32, &ring, 0) != 0) { printf("io_uring unavailable\n"); return 0; }
    start = now();
    for (int r = 0; r < ROUNDS; r++) {
      for (int i = 0; i < WRITES; i++) at[i] = (off_t)((r * 131 + i * 811) % 8192) * PAGE;
      for (int i = 0; i < WRITES; i++) {
        struct io_uring_sqe *sqe = io_uring_get_sqe(&ring);
        io_uring_prep_write(sqe, fd, buf[i], PAGE, at[i]);
      }
      if (io_uring_submit_and_wait(&ring, WRITES) != WRITES) return 3;
      struct io_uring_cqe *cqe;
      for (int i = 0; i < WRITES; i++) {
        if (io_uring_wait_cqe(&ring, &cqe) != 0 || cqe->res != PAGE) return 4;
        io_uring_cqe_seen(&ring, cqe);
      }
    }
    double uring = (now() - start) / ROUNDS;
    io_uring_queue_exit(&ring);

    printf("pass %d: %d writes of 4 KiB: pwrite %.2f us, io_uring %.2f us\n", pass, WRITES,
           plain / 1000, uring / 1000);
  }
  return 0;
}
