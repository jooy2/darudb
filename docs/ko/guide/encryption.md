---
title: 암호화
order: 7
---

# 암호화

키나 비밀번호로 만든 데이터베이스는 모든 페이지가 암호화되고 인증되며, 같은 키나 비밀번호로만 열립니다.

## 데이터베이스 암호화하기

::: lang rust

```rust
use darudb::OpenOptions;

fn main() -> Result<(), darudb::Error> {
    let db = OpenOptions::new()
        .password("correct horse battery staple")
        .open("secret.darudb")?;

    println!("{}", db.is_encrypted()); // true
    db.set_password("a new password")?;
    db.close()
}
```

비밀번호 대신 32바이트 키를 쓰려면 `OpenOptions::key`를 쓰고, 키는 `Database::set_key`로 바꿉니다.

:::

::: lang node

```ts
const db = Database.open('secret.darudb', {
  schema: app,
  password: 'correct horse battery staple'
});

console.log(db.isEncrypted); // true
db.setPassword('a new password'); // 또는 `await db.setPasswordAsync(...)`
db.close();

Database.open('secret.darudb', { schema: app }); // KEY_REQUIRED를 던집니다
```

- `key`는 32바이트 `Uint8Array`이고, `setKey`로 바꿉니다. `key`와 `password` 중 하나만 줍니다.
- `password`는 문자열이나 `Uint8Array`입니다.
- 패키지는 `open`이나 `openAsync`를 호출하는 순간 키나 비밀번호를 복사하고, 엔진이 제 사본을 가져가면 그 복사본을 지웁니다. 넘긴 `Uint8Array`는 호출이 반환되자마자 `fill(0)`으로 지워도 됩니다. 문자열은 지울 수 없어서 가비지 컬렉터가 거둘 때까지 메모리에 남습니다.
- `setKey`와 `setPassword`는 커밋을 하므로 쓰기의 규칙을 따릅니다. `Async` 짝은 이 프로세스의 다른 쓰기가 끝난 뒤 차례를 기다리고, 동기 버전은 비동기 쓰기가 파일을 쥐고 있는 동안 거부됩니다.

:::

페이지 안의 모든 것이 암호화됩니다. 키와 값, 컬렉션과 트리의 이름도 마찬가지입니다. 모든 페이지와 헤더에 기록된 커밋 정보도 인증되므로, 바이트 하나라도 바뀌면 그대로 읽지 않고 `CORRUPTED`로 알립니다. 키 없이 열면 `KEY_REQUIRED`, 틀린 키로 열면 `WRONG_KEY`로 실패합니다.

## 키와 비밀번호

- **키**는 무작위 32바이트입니다. 운영체제의 키 저장소에 보관한 키가 그런 예입니다.
- **비밀번호**는 Argon2id로 키를 만듭니다. 기본 비용은 19 MiB, 반복 2회, 병렬 1이고 수십 밀리초가 걸립니다. <LangCode rust="OpenOptions::password_hashing" node="passwordHashing" />으로 새 파일과 비밀번호 변경에 쓸 비용을 올리거나 내립니다. 파일은 만들 때의 비용을 기록해 두므로, 열 때는 옵션과 관계없이 그 비용이 듭니다.
- **키나 비밀번호를 바꿔도** 페이지를 다시 암호화하지 않고, 바꾸기가 끝나면 이전 것으로는 파일을 열 수 없습니다.
- 평문 데이터베이스는 평문으로 남고, 암호화한 데이터베이스는 키 없이 열 수 없습니다. 키나 비밀번호를 잃어버리지 않을 곳에 보관하세요. 잃어버리면 데이터를 읽을 방법이 없습니다.

페이지를 어떻게 암호화하고 키를 파일에 어떻게 보관하는지는 엔진 섹션의 [암호화](../engine/encryption.md)에 있습니다.

## 암호화한 파일과 도구

암호화한 파일의 백업은 같은 키로 암호화되고, 같은 키나 비밀번호로 열립니다. 되살리기는 손상된 파일의 키나 비밀번호를 받고, 그 키나 비밀번호로 새 파일도 열립니다. [도구](./tools.md)를 보세요.
