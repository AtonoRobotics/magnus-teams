# A run's memory cap counts what it wrote

A run may write at most `MAX_MEMORY_WRITES_PER_RUN` memories. The check counted the run's memories still kept, so forgetting or deleting one of them made room for another, and a run could write past the cap. It now counts the run's memory writes that were done (the model's and the memory pass's), which forgetting does not change (`tools_core::memories_written`).
