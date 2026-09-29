# Tools

The tools that ship with the library, for a file that has to be checked, copied, made smaller or rescued: the integrity check, backup, compaction and salvage. [Commits and recovery](commits-and-recovery.md#checking-and-salvaging) says what the integrity check verifies and what the format gives salvage to work with; this document says how each tool works and what it promises.

Every tool reads the commit a read transaction sees, or writes a new file of its own, so none of them stops other handles and processes from reading and writing the file while it runs.

## The integrity check

`Database::check` verifies the commit a new read transaction sees, which is the published commit when the check begins. It holds that transaction for as long as it runs, so no page the commit reaches is reused under it, whatever other processes commit meanwhile.

It reads, in this order:

1. **Every tree of the commit.** The catalog, the free tree and the retained tree from the commit record, then every tree the catalog names. Each page is loaded as a read loads it, which verifies it against its pointer: its check, its kind, its level, its tree and the commit that wrote it. On the way down, each page is given the separators on its path, and every key it holds has to lie at or above the lower one and below the upper one. Every overflow run a leaf names is read whole, which verifies each of its pages and the run's check.
1. **The allocator trees' entries.** Each free run and each retained run lies inside the file, and every retained group was made by a commit no newer than the one checked.
1. **The counts.** Each tree holds as many entries as its descriptor in the catalog counts, and a descriptor counts entries exactly when it names a root.
1. **The object layer**, in a file with a stored schema. Every record decodes under its collection's fields, and is stored under the key its own key field encodes to. Every entry the objects give each index is in that index, naming the right object, and each index holds as many entries as its objects give it, so it holds nothing else. Each auto-increment counter lies past every key in its collection.
1. **The accounting.** Every page of the file from 1 to the page count − 1 is used by exactly one tree or overflow run, or is free, or is retained. A page claimed twice, or claimed while outside the file, is a problem where it is claimed; a run of pages nothing claims has leaked.

The check reports every problem it finds rather than stopping at the first. A problem names the page it is in, where it is in one, and the tree or collection, where it was found in one. A page that cannot be read is a problem, and the pages below it are not read; the pages the check could not reach are then counted in one problem rather than reported as leaked, since some of them lie below the damage. The check fails only when it cannot begin, as when the database is closed; damage is what its report is for.

**What it costs.** It reads every page the commit reaches, and every record twice over for a file with indexes: once in its collection and once for each index entry it gives, looked up by key. It keeps one bit for every page of the file, and nothing that grows with the number of objects, so a file of any size can be checked in bounded memory.
