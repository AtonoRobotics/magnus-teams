# The fast tier's Postgres has pgvector

The fast tier's Postgres service is `pgvector/pgvector:pg16` (Postgres 16 with pgvector), not plain `postgres:16`.

Reason: the server needs pgvector (the knowledge index, `log.rs`), and so does Strix Beta's test Postgres. The backup tests restore into a fresh database the way the server does, and that sets up the whole schema, pgvector included. On plain Postgres those tests failed on the fast tier while passing on Strix Beta. The fast tier should test against the same database the server runs on.
