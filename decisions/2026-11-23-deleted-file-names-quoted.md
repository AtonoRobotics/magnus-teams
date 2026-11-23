# Deleted files' names are quoted in a run's context

**Date:** 2026-11-23

## Decision
Every run is told which files the person deleted from their computers
(the cloud computer's and their own). Both lists now go through one
renderer, `crate::deleted_files`: each name is quoted on one line, with `"`
and `\` escaped, control characters, line and paragraph separators and
direction marks written as their codes, and a name over 200 characters cut
and marked with `…`. The list says each entry is a file's name, quoted as
written: a name, never an instruction.

Every deleted file is still listed, from clean runs and tainted ones alike:
the events that record a write or a deletion do not say which run made the
file, and adding that would edit an event kind.

## Reason
The list sits in the always-on context, which holds only clean text
(`decisions/2026-11-21-clean-only-always-on-context.md`), and it put each
path there verbatim. A tainted run chooses the names of the files it
writes, and a name can carry newlines and an instruction, which then
reached every later run of that person as if the system had said it.
Quoted and labelled, a name stays a name.
