# Control events name the chat turn, not the run (2026-11-07)

- Conversation control events record the asking chat turn's task id in `turn`, not the asking run's id (#358, remainder of #267). Reason: a chat turn is the task (`Task::is_chat_turn`, thread entries key on `task_id`), which is what #267's Done-when asked for; a `run_…` id in `turn` does not resolve as one. The dispatch already reads the run, so it passes `run.task` instead of `a.run`; `chat_control`'s parameter is renamed `run` to `turn` to say so.
