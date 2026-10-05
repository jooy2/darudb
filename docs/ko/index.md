---
layout: home

title: DaruDB
titleTemplate: 로컬 파일 하나에 담는 임베디드 데이터베이스
description: 애플리케이션의 데이터를 로컬 파일 하나에 담는 임베디드 데이터베이스입니다. Rust로 작성한 엔진 하나를 Rust와 Node.js, Dart에서 함께 쓰며, 암호화와 크래시 안전성, 여러 프로세스의 동시 접근을 처음부터 설계에 담았습니다.

hero:
  name: DaruDB
  text: Rust와 Node.js, Dart를 위한 임베디드 데이터베이스
  tagline: Rust로 작성한 엔진 하나가 애플리케이션의 데이터를 로컬 파일 하나에 담습니다. 암호화와 크래시 안전성, 여러 프로세스가 파일 하나를 함께 쓰는 일은 나중에 덧붙이지 않고 처음부터 설계에 넣었습니다.
  image:
    src: /logo.webp
    alt: DaruDB 로고
  actions:
    - theme: brand
      text: 소개
      link: /ko/guide/introduction
    - theme: alt
      text: 시작하기
      link: /ko/guide/getting-started
    - theme: alt
      text: GitHub
      link: https://github.com/jooy2/darudb

features:
  - title: 어느 언어에서나 같은 엔진
    details: 엔진은 Rust로 한 번만 작성하고, 각 언어는 얇은 바인딩으로 그 엔진을 씁니다. 파일에 관한 규칙이 모두 한곳에 있으므로 Node.js에서 쓴 파일을 Rust나 Dart에서 읽어도 똑같이 읽힙니다.
    link: /ko/guide/introduction
    linkText: 구조 살펴보기
  - title: 파일 전체 암호화
    details: 파일의 모든 페이지를 암호화하고 인증합니다. 키 없이 읽으면 아무것도 보이지 않고, 키 없이 고치면 바로 드러납니다. 비밀번호를 바꿔도 파일 전체를 다시 쓰지 않습니다.
  - title: 크래시에도 살아남는 파일
    details: 커밋한 페이지는 제자리에서 덮어쓰지 않고, 커밋은 바이트 하나를 바꾸는 순간 반영됩니다. 쓰는 도중 프로세스가 죽거나 전원이 나가도 마지막 커밋은 그대로 남습니다.
  - title: 여러 프로세스가 파일 하나를
    details: 프로세스끼리는 공유 메모리 없이 운영체제의 파일 잠금만으로 조율합니다. 잠금을 쥔 프로세스가 죽어도 나머지가 멈춰 서지 않습니다.
---

::: info 진행 상황

위 네 가지는 설계가 향하는 목표이고, 각 목표를 증명하는 테스트와 벤치마크가 저장소에 들어온 뒤에야 기능이라고 말할 수 있습니다. Rust 크레이트와 Node.js, Dart 패키지를 모두 배포했습니다. 지금 동작하는 범위는 [현재 상태](/ko/guide/introduction#현재-상태)에 정리했습니다.

:::
