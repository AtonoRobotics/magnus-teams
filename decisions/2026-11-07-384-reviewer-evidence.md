# Completion reviewer sees the run's evidence (#384)

Codex P1 on #380: the completion reviewer judged from the request,
goal, criteria and agent summary alone, yet a hold recorded
verification. The prompt now carries the run's tool calls with each
outcome and return, newest last, each clipped to 400 chars and 8000
total (earlier results omitted past that). Results may hold untrusted
content; the prompt already reads everything as data, as the verifier
does when it reads the world.
