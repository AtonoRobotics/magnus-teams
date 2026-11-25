# A person's agent in a project's channel files nothing into their memory

**Choice.** The memory pass does not run for a person's own agent at work in
a project's channel: a member's lane asked there, or a whisper there, and
any sub-agent of either (`State::in_project_channel`). It still runs
everywhere else.

**Why.** It was verified, not assumed (`tests/scratch/tests/project_lane_memory.rs`).
A member's lane in a project's channel answered from the project, and the
memory pass filed that answer as a fact in the member's personal memory.
The fact stayed there after the member was removed from the project. A
project is read only by its people (the isolation model), and personal
memory outlasts a person's place in a project, so nothing from one may be
filed there. Filtering what the pass files would mean judging which facts
came from the project. Not running the pass is the simplest rule that is
always right. A whisper in a project's channel sees that channel's messages,
which are the project's too, so the same rule covers it.
