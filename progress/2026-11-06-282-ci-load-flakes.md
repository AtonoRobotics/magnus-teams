# CI load flakes behind the red master (2026-11-06)

Master went red twice on flakes seen only under overlapping full suites: the sign-in page test's 12-second bound (12.6 s, 14.0 s observed) and the mail server's `docker run` not answering in 180 seconds. The bound is 18 seconds with the runs cited, and the mail container start retries twice under a per-attempt name. No product code changed.
