---
layout: home

title: DaruDB
titleTemplate: An embedded database in one local file
description: An embedded database that keeps an application's data in one local file, for Rust, Node.js and Dart. One engine written in Rust, with encryption, crash safety and several processes on one file designed in from the start.

hero:
  name: DaruDB
  text: An embedded database for Rust, Node.js and Dart
  tagline: One engine, written in Rust, keeps your application's data in a single local file. Encryption, crash safety and several processes sharing one file are designed in from the start, not added later.
  actions:
    - theme: brand
      text: Introduction
      link: /guide/introduction
    - theme: alt
      text: Getting started
      link: /guide/getting-started
    - theme: alt
      text: GitHub
      link: https://github.com/jooy2/darudb

features:
  - title: One engine in every language
    details: The engine is written once, in Rust, and each language reaches it through a thin binding. A file written from Node.js reads the same from Rust or Dart, because every rule about the file lives in one place.
    link: /guide/introduction
    linkText: How it is built
  - title: Encryption of the whole file
    details: Every page of the file is encrypted and authenticated, so a file read without its key shows nothing and a file changed without it is detected. Changing a password does not rewrite the file.
  - title: A file that survives a crash
    details: A committed page is never overwritten in place, and a commit becomes visible by flipping a single byte. A process killed mid-write, or a power cut, leaves the last commit intact.
  - title: Several processes on one file
    details: Processes coordinate through operating-system file locks alone, never shared memory, so a process that dies holding a lock cannot leave the others stuck.
---

::: warning Early development

DaruDB is in early development. The four cards above are the goals the design is built around, and each becomes a claim only when the tests and benchmarks that prove it are in the repository. Nothing is published yet, and the file format will change without a migration until the first release. [Where it stands](/guide/introduction#where-it-stands) says what works today.

:::
