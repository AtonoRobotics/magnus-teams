# Checks that sit outside the working model

**Login codes never reach the agent.** A connector in the email category (`filter_login_codes`) has its results passed through `login_codes::strip` before anything else reads them: one-time codes, password reset links and sign-in links are replaced with a note and the rest of the message stays. It runs before the result is logged, so the log never holds one.

**Outgoing classifier.** Every outgoing action that Auto Review sees is first scanned by a classifier call, and the finding goes into the review document as `outgoing_classifier_finding`. If the classifier fails, the action is asked about, not sent. A failed check does not count as a clean check.

**Inbound screening.** Text from outside is flagged before the model reads it, with a line saying it was screened on arrival. It is a phrase heuristic with no model behind it, and it does not read images. It can only warn. Taint is what limits what follows.

**An independent monitor.** `[safety] monitor_model` names a different model that sees each reviewed send and answers allow, block, review or pause. It is off unless set, because a monitor running on the working model adds no independence. It does not watch inference traffic, only reviewed sends.
