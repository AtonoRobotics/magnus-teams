# The dev and test Postgres (`magnus-claude-pg`): the image it already ran,
# by digest, with pgvector from the same PGDG repository, so the data
# directory, glibc and collations are unchanged.
FROM postgres@sha256:287eced1f33b59ed265ed13a60d3680dd7646d70c4dc0e785f59a470ebc03eeb
ARG PGVECTOR=0.8.7-1.pgdg13+1
RUN apt-get update && apt-get install -y --no-install-recommends postgresql-16-pgvector=${PGVECTOR} && rm -rf /var/lib/apt/lists/*
