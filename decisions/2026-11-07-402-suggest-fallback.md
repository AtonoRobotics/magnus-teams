# Fallback observations need suggest leave (#402)

Codex P2 on #401: with `suggest` denied but `interrupt` allowed, an
interruption outside working hours or over budget still queued an
observation — the fallback branches bypassed the gate that only
guarded the interrupt-denied path.

Both fallback branches now check `suggest`: denied means nothing is
queued (an error result naming the denial), mirroring the direct
observe arm. Every path that creates an observation goes through
the suggest check.
