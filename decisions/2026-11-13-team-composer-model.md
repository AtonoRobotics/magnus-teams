# The team chat's work card and model picker follow the personal chat

Two visible differences were left after one chat view (2026-11-13-one-chat-view.md).

**A work card after its answer.** The personal chat shows a turn's line while
Magnus works and replaces it with the answer. The team chat kept a "Plan the
launch · Done" card under the request after the answer came. Now a short
request's card goes once it has an answer (completed, or failed with its "I
could not finish this" answer); the answer stands in its place. Long work keeps
its row with a link to its thread, as a task a personal message started keeps
its card. Work stopped with no answer (cancelled, suspended) keeps its line, as
a stopped personal turn does. No spec line asks the team chat to keep a done
card, so the personal behaviour stands. Held in
`team::items::timeline` and `a_short_requests_card_gives_way_to_its_answer`.

**Model picker.** A team request is the requesting member's own run: their
model access and monthly spend apply, and the workspace's spend limit caps its
budget. No admin sets a team agent's model; the server's default is used, as
for a personal message. So the picker is the personal composer's, first in the
bar, and a channel message or whisper carries `model` as `POST /v1/chat` does,
checked by the same `api::chosen_model` (a model no adapter offers is refused
before anything is posted). A guest's messages never start the agent, so a
guest sees the picker disabled with that reason rather than hidden.
