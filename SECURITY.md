# Security Policy

## Supported versions

Only the **latest release** of each package receives security fixes; there are no maintained release branches behind it, so upgrading is how a fix arrives.

| Package              | Version        | Supported |
| -------------------- | -------------- | --------- |
| `darudb` (crates.io) | Latest release | Yes       |
| `darudb` (crates.io) | Anything older | No        |
| `darudb` (npm)       | Latest release | Yes       |
| `darudb` (npm)       | Anything older | No        |
| `darudb` (pub.dev)   | Latest release | Yes       |
| `darudb` (pub.dev)   | Anything older | No        |

If a release line is ever maintained on its own, this table will name it.

## Reporting a vulnerability

**Do not open a public issue for a security problem**, and do not describe one in a pull request. A vulnerability that is public before there is a version to upgrade to puts every application that stores its data in this database at risk.

Report it privately through one of these routes:

- **GitHub Security Advisories.** [Open a draft advisory](https://github.com/jooy2/darudb/security/advisories/new). This is the preferred route: it keeps the report, the fix and the eventual disclosure in one place.
- **Email.** [jooy2.contact@gmail.com](mailto:jooy2.contact@gmail.com), with `darudb security` in the subject.

Please include, as far as you can:

- The version of the package, the language binding, and the operating system and file system it was reproduced on.
- What an attacker can do with it: read data without the key, change data without it being detected, crash the process, or read memory it should not. Say that rather than only what the input looks like.
- The smallest program or database file that reproduces it, and the steps to get there. If the file itself is the attack, attach it to the advisory rather than describing it.
- Whether it is already public anywhere.

### What happens next

- **Within 3 days** you should have an acknowledgement that the report arrived.
- **Within 14 days** you should have an assessment: whether it is accepted, what the impact is judged to be, and a rough timetable.
- A fix is released as soon as it is ready, and the advisory is published with it. If you would like to be credited, say so in the report and name what you would like to be credited as.

If you do not hear back inside those windows, please follow up rather than assume the report arrived.

## Scope

This project is an embedded database that keeps an application's data in a local file, so the things most worth reporting are the ones that break what the file is trusted to do:

- **Encryption.** Reading any part of an encrypted database without its key, telling one encrypted value from another where the design says that should not be possible, recovering a key or a password from the file, or a weakness in how a key is derived from a password.
- **Integrity.** Changing an encrypted database's contents without the change being detected when the file is next read.
- **Untrusted files.** A crafted or damaged database file that makes the library crash, hang, allocate without bound, or read or write memory it should not, when the file is opened, checked or repaired.
- **Several processes on one file.** A sequence of operations from two processes that loses a committed write or leaves the file in a state the recovery on open cannot repair.
- **Supply chain.** A vulnerability in something a published package actually depends on at runtime, reachable through the way we use it.

Out of scope:

- An attacker who can already write to the process's memory or run code in it. The key is in that memory while the database is open.
- Data loss on a file system the documentation says is not supported, such as a network file system, or on hardware that acknowledges a write before it has stored it.
- Reports about a **development dependency** that never reaches a published package, unless you can show a path from it into a published artifact.
- Findings from an automated scanner with no demonstrated impact.
- Anything about the documentation site's hosting rather than the library.

## Disclosure

We ask for coordinated disclosure: give us a chance to release a fix before the details are public. In return, we answer within the timetable above, and if a fix will take longer than that, we will say so and why.
