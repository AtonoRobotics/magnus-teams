# One way to make a paid model call

**Choice.** Every model call outside the agent loop's own step goes through
`Kernel::paid_call` (`spend::paid`): the reviewer and the checks on its
model (Auto Review, the outgoing classifier, envelope and quarantined
extraction), the memory pass, knowledge extraction, the spoken script, the
safety monitor, the inbound and tool-definition screens, and the
credential review. `Adapters::complete` is called nowhere else. It:

1. refuses a model whose provider an admin turned off, and one the person
   may not use, unless the server set it for its own checks (`[safety]`,
   the default model for a credential review): a person's model access
   rules their choices, not the server's checks;
2. fits the call's whole worst case, at the adapter's quote, to what the
   run has left, or to what its owner has left when there is no run: a
   person's month, a workspace's month (a workspace computer's credential
   review), or the server's month (a tool definition screened outside any
   run). A call that cannot be priced goes on only where no limit applies;
3. makes the call;
4. records its cost whether it answers or fails: a run's call as its
   caller's own usage event (`reviewer_reply`, `memory_pass`, ...), a
   failed one the provider charged for, or any call for no run, as the new
   `spend` change `charged`. Each lands on the money ledger.

The safety monitor and the screens are a new spender, `Check`: bounded by
dollars, not by the call cap, so a run's last step is still checked. A
check refused for want of allowance or a model fails safe, as each caller
already does on a timeout: the monitor holds the work for the person (and
says it could not check), an inbound screen withholds the content, a tool
definition stays unscreened, a credential stays home, the outgoing
classifier and Auto Review ask the person. The reviewer, memory pass,
extraction and spoken script skip, as they did.

A provider that is off is refused by the model check, the run's model
choice (never the default or the first allowed either), escalation at
start and when a lost run escalates, the fallback switch, and before each
of the agent loop's attempts, which moves to a fallback it may use or
pauses the task.

**Reason.** The monitor, the screens and the credential review called the
provider with no allowance check and no record, so their cost passed every
limit unseen; tool screens outside a run were never recorded; four callers
dropped what a failed call had been charged; and the provider switch was
read only by the model picker.
