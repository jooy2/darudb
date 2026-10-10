---
title: Constants
order: 13
group: errors
pageClass: reference-page
---

# Constants

이 크레이트는 크레이트의 버전과, 읽고 쓰는 가장 새 파일 형식 버전을 상수 두 개로 내놓습니다.

## VERSION

```rust
pub const VERSION: &str
```

`darudb` 크레이트의 버전으로, `Cargo.toml`에 적힌 `"1.1.0"` 같은 문자열입니다. 프로그램에 컴파일돼 들어간 라이브러리의 버전이라 로그나 버그 제보에 적어 두기 좋습니다.

## FORMAT_VERSION

```rust
pub const FORMAT_VERSION: u32
```

이 빌드가 읽고 쓰는 가장 새 파일 형식 버전인 6이며, 새 파일은 이 버전으로 만듭니다. 파일마다 헤더에 형식 버전을 기록하고 [`Database::format_version`](../../api/rust/database.md#format-version)이 그 값을 돌려줍니다. 이 빌드가 읽지 않는 버전의 파일은 [`UNSUPPORTED_FORMAT_VERSION`](./error.md#unsupportedformatversion)으로 거부하며, 그 오류의 `found` 필드가 파일의 버전입니다.

디스크에 쓰는 내용이 바뀌면 이 수도 바뀌고, 버전 6부터는 모든 형식 버전이 바로 앞 버전에서 옮겨 오는 마이그레이션과 함께 나옵니다. 첫 릴리스의 형식은 버전 5이고, 버전 6은 리프 항목의 길이를 더 적은 바이트로 적습니다. 이 빌드는 두 버전을 모두 읽고 쓰며, 버전 5 파일을 열면 [`OpenOptions::upgrade_format`](../../api/rust/open-options.md#upgrade-format)이 막지 않는 한 버전 6으로 올립니다. 버전 5보다 앞선 개발 빌드가 쓴 파일은 열리지 않습니다. 데이터베이스의 객체는 따로 정한 형식으로 인코딩하고 그 버전은 저장된 스키마에 기록하므로, 이 상수는 객체 형식을 다루지 않습니다. 이 버전이 무엇을 다루는지는 [파일 형식](../../engine/file-format.md)에 있습니다.

```rust
fn main() {
    println!("DaruDB {} (file format {})", darudb::VERSION, darudb::FORMAT_VERSION);
}
```
