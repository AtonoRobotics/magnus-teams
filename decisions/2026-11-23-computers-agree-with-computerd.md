# Computers agree with the computer service; one place stops what work used

## Making and deleting (`computer/fleet.rs`)

Deleting a computer recorded it gone, then asked computerd once. If computerd
was restarting, the computer, its volume and its slot stayed for ever: the
server's capacity fell, and after an erasure the person's volume outlived the
purge. Magnus never asked computerd what it had.

Now one module makes and deletes computers. A computer is made by computerd,
then recorded; if the record fails it is deleted again (now, or at the next
look). A deletion is recorded first: the log forgets the computer and keeps
its id among the computers it deleted (the ledger's `deleted`, rebuilt by
replay from each `killed` that deleted a volume). Its secrets, last screen and
download record go at once, then computerd is asked. Every minute, before the
usual look for owners without a computer, Magnus lists computerd's computers
and deletes every one the log deleted; a deletion computerd missed finishes
there. The first look after this ships also deletes any computer an earlier
missed deletion left behind.

The look never deletes a computer the log did not delete, even one the log
has never heard of: if the log were ever misread, deleting what it does not
name could take everyone's files. Such a computer, and one the log has that
computerd lacks, is only logged.

## Stopping what work used (`computer/halt.rs`)

The kill switches paused the agent's computers and closed the host browser's
pages, but left browser computers running: throwaway ones and those signed in
as the person kept running page scripts and reaching the network until the
idle stop ten minutes later, and a run continued later landed on the same
page. `stop_work` now does all of it for every switch: the host browser's
pages, the runs' sandboxes, the throwaway browsers of the stopped runs and of
the owners it reaches (deleted, so a continued run starts on a blank page,
as with the host browser), the reached owners' signed-in browsers (stopped,
their profiles sealed), and their computers (paused and held). A run's end
goes through `release_run`: its pages, its throwaway browser and its sandbox.

## Time from the log

The 24-hour run limit and the idle clock were counted in memory, so every
release started them again: a computer that ran for days never reached the
limit. The run limit now counts from the time the log recorded the computer
starting. The idle clock counts from the later of a use seen by this process
and the log's last record of the computer, so after a restart a computer is
idle since its last logged activity, not since the restart.

## The wake queue

A computer that failed to start at the head of the queue for any reason other
than room stayed at the head, and nobody behind it started. It now leaves the
queue with a new `wake_failed` change (status stopped); its next use wakes it
directly and says why if it still cannot. A computer its person paused while
it waited had no VM to resume, so Start failed for ever; Start now asks
computerd, and a computer whose VM is not paused is recorded stopped and woken.
