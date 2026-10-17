---
title: 암호화
order: 9
---

# 암호화

키나 비밀번호로 만든 데이터베이스는 모든 페이지가 암호화되고 인증되며, 같은 키나 비밀번호로만 열립니다.

## 파일을 암호화하는 방식

암호화한 데이터베이스를 만들면 엔진은 무작위 32바이트로 데이터 키를 만들고, 그 뒤로 쓰는 모든 페이지를 이 키로 봉인합니다. 사용자가 주는 키나 비밀번호로 페이지를 직접 암호화하지는 않습니다. 그 키는 데이터 키를 감싸는 데만 쓰이고, 파일에는 감싼 데이터 키만 저장됩니다.

- **페이지.** 페이지는 인증 암호인 XAES-256-GCM이나 XChaCha20-Poly1305로 암호화하고, 24바이트 논스는 페이지를 쓸 때마다 새로 뽑습니다. 어느 쪽을 쓸지는 파일을 만드는 컴퓨터가 자기 프로세서에서 빠른 쪽으로 골라 파일에 기록합니다. 어느 쪽으로 만든 파일이든 어떤 컴퓨터에서나 열립니다.
- **검사.** 암호의 16바이트 인증 태그가 곧 페이지의 검사값이고, 페이지 안과 그 페이지를 가리키는 포인터에 함께 들어 있습니다. 누가 바꾼 페이지나 예전 사본으로 되돌려 놓은 페이지는 읽지 않고 `CORRUPTED`로 실패합니다. 헤더의 커밋 레코드에도 데이터 키에서 끌어낸 키로 만든 MAC이 붙습니다.
- **데이터 키.** 사용자의 32바이트 키, 또는 비밀번호를 Argon2id로 해시한 값이 데이터 키를 감싸고, 감싼 데이터 키는 모든 커밋 레코드에 저장됩니다. 키나 비밀번호를 바꾸면 같은 데이터 키를 다시 감쌀 뿐이라 페이지를 다시 암호화하지 않습니다.
- **가려지는 것과 보이는 것.** 키와 값, 컬렉션과 트리의 이름은 모두 페이지 안에 있어서 암호화됩니다. 헤더 페이지는 평문이라 페이지 크기, 암호 방식, 트랜잭션 ID, 파일 크기는 키 없이도 읽힙니다.
- **메모리.** 엔진은 파일에서 페이지를 읽을 때 복호화하고 평문을 페이지 캐시에 둡니다. 그래서 캐시에서 바로 답하는 읽기는 평문 파일과 비용이 같습니다.

<Diagram name="encryption" alt="비밀번호는 Argon2id를 거치고, 32바이트 키는 그대로 쓰여 키 암호화 키가 됩니다. 이 키는 저장하지 않습니다. 키 암호화 키는 데이터 키를 감싸고, 무작위로 만든 데이터 키는 감싼 채로 모든 커밋 레코드에 저장됩니다. 데이터 키는 XAES-256-GCM이나 XChaCha20-Poly1305와 새 논스로 모든 페이지를 봉인합니다. 비밀번호나 키를 바꾸면 같은 데이터 키를 다시 감쌀 뿐이라 페이지는 다시 암호화하지 않습니다." />

자세한 내용은 엔진 섹션의 [암호화](../engine/encryption.md)에 있습니다. 두 암호 방식을 고른 이유, 감싼 데이터 키를 저장하고 찾는 방법, 커밋 레코드에 MAC이 필요한 이유를 다룹니다.

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
- `setKey`와 `setPassword`는 커밋을 하므로 쓰기의 규칙을 따릅니다. `Async` 짝은 이 프로세스가 같은 파일에 하는 다른 쓰기가 끝난 뒤 차례를 기다리고, 동기 버전은 비동기 쓰기가 파일을 쥐고 있는 동안 거부됩니다.

:::

::: lang dart

```dart
final db = Database.open(
  'secret.darudb',
  schema: const Schema(1, [userSchema]),
  password: 'correct horse battery staple',
);

print(db.isEncrypted); // true
db.setPassword('a new password'); // 또는 `await db.setPasswordAsync(...)`
db.close();

Database.open('secret.darudb', schema: const Schema(1, [userSchema])); // KEY_REQUIRED를 던집니다
```

- `key`는 32바이트 `Uint8List`이고, `setKey`로 바꿉니다. `key`와 `password` 중 하나만 줍니다.
- 패키지는 `open`이나 `openAsync`를 호출하는 순간 키나 비밀번호를 네이티브 메모리로 복사하고, 엔진이 제 사본을 가져가면 그 복사본을 지웁니다. 넘긴 `Uint8List`는 호출이 반환되자마자 `fillRange`로 지워도 됩니다. `String`은 지울 수 없어서 가비지 컬렉터가 거둘 때까지 메모리에 남습니다.
- `setKey`와 `setPassword`는 커밋을 하므로 쓰기의 규칙을 따릅니다. `Async` 짝은 이 isolate가 같은 파일에 하는 다른 비동기 쓰기가 끝난 뒤 차례를 기다리고, 동기 버전은 비동기 쓰기가 파일을 쥐고 있는 동안 거부됩니다.

:::

::: lang python

```python
import darudb

db = darudb.Database.open(
    "secret.darudb",
    schema=darudb.Schema(1, [User]),
    password="correct horse battery staple",
)

print(db.is_encrypted)  # True
db.set_password("a new password")  # 또는 `await db.set_password_async(...)`
db.close()

darudb.Database.open("secret.darudb", schema=darudb.Schema(1, [User]))  # KEY_REQUIRED를 일으킵니다
```

- `key`는 32바이트이고 `bytes`나 `bytearray`, `memoryview`로 주며, `set_key`로 바꿉니다. `key`와 `password` 중 하나만 줍니다.
- `password`는 `str`이나 바이트입니다.
- 네이티브 모듈은 키나 비밀번호를 자기 버퍼로 복사하고, 엔진이 제 사본을 가져가면 그 버퍼를 지웁니다. 넘긴 `bytearray`는 호출이 반환된 뒤에, `_async` 짝이라면 await가 끝난 뒤에 `key[:] = bytes(len(key))`로 지워도 됩니다. `bytes`와 `str`은 지울 수 없어서 Python이 메모리를 해제할 때까지 남습니다.
- `set_key`와 `set_password`는 커밋을 하므로 쓰기의 규칙을 따릅니다. `_async` 짝은 이 이벤트 루프가 같은 파일에 하는 다른 비동기 쓰기가 끝난 뒤 차례를 기다리고, 동기 버전은 비동기 쓰기가 파일을 쥐고 있는 동안 루프의 스레드에서 거부됩니다.

:::

키 없이 열면 `KEY_REQUIRED`, 틀린 키로 열면 `WRONG_KEY`로 실패합니다. 평문 파일을 키나 비밀번호와 함께 열면 `INVALID_ARGUMENT`로 실패합니다.

## 키와 비밀번호

- **키**는 무작위 32바이트입니다. 파일이 곧바로 열리므로, [아래](#운영체제-키-저장소에-키-보관하기)처럼 운영체제의 키 저장소에 사용자 대신 키를 보관하는 앱이나, 다른 비밀 값과 함께 키를 두는 서버에 맞습니다.
- **비밀번호**는 파일을 열 때마다 Argon2id로 키를 만듭니다. 기본 비용은 19 MiB, 반복 2회, 병렬 1이고 수십 밀리초가 걸립니다. 사람이 직접 입력해서 여는 파일에 맞습니다. <LangCode rust="OpenOptions::password_hashing" node="passwordHashing" dart="passwordHashing" python="password_hashing" />으로 새 파일과 비밀번호 변경에 쓸 비용을 올리거나 내립니다. 파일은 만들 때의 비용을 기록해 두므로, 열 때는 옵션과 관계없이 그 비용이 듭니다.
- 한쪽으로 만든 파일을 다른 쪽으로 바꿀 수 있습니다. 비밀번호로 만든 파일에 <LangCode rust="Database::set_key" node="setKey" dart="setKey" python="set_key" />로 키를 주거나, 반대로 <LangCode rust="Database::set_password" node="setPassword" dart="setPassword" python="set_password" />로 비밀번호를 줍니다.

## 키 만들기

키는 무작위여야 합니다. 안전한 난수 생성기에서 얻은 32바이트를 쓰고, 이름이나 날짜, 비밀번호에서 계산한 바이트는 키로 쓰지 않습니다. 앱은 [다음 절](#운영체제-키-저장소에-키-보관하기)처럼 코드에서 키를 만듭니다. 테스트에 쓸 키나 서버가 비밀 값과 함께 보관할 키라면 아래 버튼으로 만들 수 있습니다.

<KeyGenerator />

키는 이 브라우저 안에서 Web Crypto API의 `crypto.getRandomValues`로 만들고, 어디로도 보내지 않습니다. 두 형식에는 같은 32바이트가 들어 있으며, 16진수로는 64자, Base64로는 44자입니다. 서버가 키를 환경 변수에 둔다면 데이터베이스를 열기 전에 디코딩합니다. 아래 예제는 Base64로 담은 `DATABASE_KEY`를 읽습니다.

::: lang rust

```rust
use base64::Engine as _;

fn database_key() -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let text = std::env::var("DATABASE_KEY")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(text)?;

    Ok(bytes.try_into().map_err(|_| "DATABASE_KEY is not 32 bytes")?)
}
```

표준 라이브러리에는 Base64 디코더가 없어서 `base64` 크레이트를 씁니다.

:::

::: lang node

```ts
const db = Database.open('app.darudb', {
  schema: app,
  key: Buffer.from(process.env.DATABASE_KEY ?? '', 'base64')
});
```

32바이트로 디코딩되지 않는 키는 `INVALID_ARGUMENT`로 실패합니다.

:::

::: lang dart

```dart
import 'dart:convert';
import 'dart:io';

final db = Database.open(
  'app.darudb',
  schema: const Schema(1, [userSchema]),
  key: base64Decode(Platform.environment['DATABASE_KEY']!),
);
```

32바이트로 디코딩되지 않는 키는 `INVALID_ARGUMENT`로 실패합니다.

:::

::: lang python

```python
import base64
import os

db = darudb.Database.open(
    "app.darudb",
    schema=darudb.Schema(1, [User]),
    key=base64.b64decode(os.environ["DATABASE_KEY"]),
)
```

32바이트로 디코딩되지 않는 키는 `INVALID_ARGUMENT`로 실패합니다.

:::

## 운영체제 키 저장소에 키 보관하기

앱이 한 번 만든 키를 운영체제의 키 저장소에 두면, 사용자에게 비밀번호를 묻지 않고도 파일을 암호화할 수 있습니다. 처음에 무작위 32바이트를 만들어 저장하고, 그다음부터는 그 키로 데이터베이스를 엽니다. DaruDB가 키 저장소를 직접 다루지는 않으므로 아래 코드는 플랫폼마다 제공하는 기능을 씁니다. 어느 앱과 사용자가 항목을 읽을 수 있는지는 키 저장소마다 규칙이 다릅니다. 첫 커밋 전에 키를 저장하세요. 키를 잃으면 데이터도 잃습니다.

::: lang rust

```rust
use darudb::OpenOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut key = [0u8; 32];

    getrandom::fill(&mut key)?;
    // 여기서 `key`를 키 저장소에 보관하고, 다음부터는 거기서 읽습니다.

    let db = OpenOptions::new().key(key).open("secret.darudb")?;

    db.close()?;

    Ok(())
}
```

`keyring` 크레이트로 macOS와 iOS의 키체인, Windows의 자격 증명 관리자, Linux의 Secret Service에 접근할 수 있습니다. 서버라면 다른 비밀 값과 같은 곳에 키를 둡니다. 배포 과정이 비밀 관리 서비스에서 채워 주는 환경 변수가 그런 예입니다.

:::

::: lang node

Electron의 메인 프로세스에서는 `safeStorage`가 운영체제가 보관하는 키로 값을 암호화합니다. macOS는 키체인, Windows는 DPAPI, Linux는 데스크톱의 비밀 저장소를 씁니다. 이렇게 암호화한 데이터베이스 키는 데이터베이스 옆 파일에 둬도 됩니다.

```ts
import { randomBytes } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { app as electronApp, safeStorage } from 'electron';

// `ready` 이벤트 뒤에 부릅니다.
function databaseKey(): Uint8Array {
  const file = join(electronApp.getPath('userData'), 'database.key');

  if (existsSync(file)) {
    return Buffer.from(safeStorage.decryptString(readFileSync(file)), 'base64');
  }

  const key = randomBytes(32);

  writeFileSync(file, safeStorage.encryptString(key.toString('base64')));

  return key;
}

const db = Database.open(join(electronApp.getPath('userData'), 'app.darudb'), {
  schema: app,
  key: databaseKey()
});
```

- 먼저 `safeStorage.isEncryptionAvailable()`을 확인하세요. Linux 데스크톱에 비밀 저장소가 없으면 `safeStorage.getSelectedStorageBackend()`가 `basic_text`를 돌려주고, 이때 암호화한 값은 사용자만 가진 무엇으로도 보호되지 않습니다.
- 서버라면 서비스의 다른 비밀 값과 같은 곳에 키를 둡니다. 배포 과정이 비밀 관리 서비스에서 채워 주는 환경 변수가 그런 예입니다: `Buffer.from(process.env.DATABASE_KEY, 'base64')`.

:::

::: lang dart

Flutter 앱에서는 `flutter_secure_storage` 패키지가 iOS와 macOS에서는 키체인에, Android에서는 Android Keystore의 키로 암호화해 값을 보관합니다.

```dart
import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

Future<Uint8List> databaseKey() async {
  const storage = FlutterSecureStorage();
  final stored = await storage.read(key: 'database-key');

  if (stored != null) {
    return base64Decode(stored);
  }

  final random = Random.secure();
  final key = Uint8List.fromList([for (var i = 0; i < 32; i++) random.nextInt(256)]);

  await storage.write(key: 'database-key', value: base64Encode(key));

  return key;
}

final db = Database.open(
  'secret.darudb',
  schema: const Schema(1, [userSchema]),
  key: await databaseKey(),
);
```

Dart 서버나 명령줄 도구라면 다른 비밀 값과 같은 곳에 키를 둡니다. 배포 과정이 비밀 관리 서비스에서 채워 주는 환경 변수가 그런 예입니다.

:::

::: lang python

`keyring` 패키지로 macOS의 키체인, Windows의 자격 증명 관리자, Linux의 Secret Service에 접근할 수 있습니다. 이 패키지는 텍스트를 저장하므로 키는 Base64로 바꿔 둡니다.

```python
import base64
import secrets

import darudb
import keyring


def database_key() -> bytes:
    stored = keyring.get_password("my-app", "database-key")

    if stored is not None:
        return base64.b64decode(stored)

    key = secrets.token_bytes(32)
    keyring.set_password("my-app", "database-key", base64.b64encode(key).decode())

    return key


db = darudb.Database.open(
    "secret.darudb",
    schema=darudb.Schema(1, [User]),
    key=database_key(),
)
```

서버라면 다른 비밀 값과 같은 곳에 키를 둡니다. 배포 과정이 비밀 관리 서비스에서 채워 주는 환경 변수가 그런 예이고, 이때는 `base64.b64decode(os.environ["DATABASE_KEY"])`로 키를 읽습니다.

:::

## 기존 데이터베이스 암호화하기

평문 데이터베이스는 그 자리에서 암호화할 수 없습니다. 키나 비밀번호를 준 백업이 암호화한 사본을 쓰고, 그 사본이 평문 파일 자리를 차지합니다.

::: lang rust

```rust
use darudb::{BackupOptions, Database};

fn encrypt(key: [u8; 32]) -> Result<(), Box<dyn std::error::Error>> {
    let db = Database::open("app.darudb")?;

    db.backup_with("app.encrypted.darudb", BackupOptions::new().key(key))?;
    db.close()?;
    std::fs::rename("app.encrypted.darudb", "app.darudb")?;

    Ok(())
}
```

:::

::: lang node

```ts
import { renameSync } from 'node:fs';

const db = Database.open('app.darudb', { schema: app });

db.backup('app.encrypted.darudb', { key }); // 또는 `await db.backupAsync(...)`
db.close();
renameSync('app.encrypted.darudb', 'app.darudb');
```

:::

::: lang dart

```dart
import 'dart:io';

final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]));

db.backup('app.encrypted.darudb', key: key); // 또는 `await db.backupAsync(...)`
db.close();
File('app.encrypted.darudb').renameSync('app.darudb');
```

:::

::: lang python

```python
import os

db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]))
db.backup("app.encrypted.darudb", key=key)  # 또는 `await db.backup_async(...)`
db.close()
os.replace("app.encrypted.darudb", "app.darudb")
```

:::

- 사본에는 백업을 시작할 때 공개된 커밋이 들어갑니다. 사본을 옮겨 놓기 전에 쓰기를 멈추고, 모든 프로세스에서 평문 파일의 핸들을 닫으세요. 그 사이에 한 쓰기는 사본에 없고, 예전 파일을 연 채로 둔 프로세스는 그 파일을 계속 씁니다.
- 평문 파일을 사본으로 바꿔도 디스크에서 지워지지는 않습니다. 파일 시스템은 그 블록을 덮어쓰지 않고 비워 두기만 하며, 그 전에 백업이나 동기화 폴더에 생긴 사본도 평문으로 남습니다.

## 키나 비밀번호 바꾸기

<LangCode rust="Database::set_key" node="setKey" dart="setKey" python="set_key" />와 <LangCode rust="Database::set_password" node="setPassword" dart="setPassword" python="set_password" />는 [데이터베이스 암호화하기](#데이터베이스-암호화하기)에서 보듯 데이터 키를 새 키나 비밀번호로 다시 감쌉니다. 페이지는 다시 암호화하지 않으므로 파일 크기와 관계없이 걸리는 시간이 같습니다. 동기 커밋 세 번이면 끝나는데, 한 번은 새로 감싼 데이터 키를 쓰고 두 번은 헤더의 나머지 슬롯에 남은 예전 것을 덮어씁니다. 호출이 반환되면 예전 키나 비밀번호로는 이 파일을 열 수 없습니다. 새 비밀번호는 핸들을 열 때 준 옵션의 비용으로 해시합니다.

바꾸기로 달라지지 않는 것도 있습니다.

- **바꾸기 전에 만든 사본.** 백업이나 운영체제 백업, 동기화 폴더에 있는 사본은 예전에 감싼 데이터 키를 그대로 갖고 있어서 예전 키나 비밀번호로 여전히 열립니다.
- **데이터 키.** 예전 비밀번호와 바꾸기 전의 파일 사본을 함께 가진 사람은 그 사본에서 데이터 키를 풀어낼 수 있고, 그 데이터 키로 지금 파일도 복호화됩니다. 비밀번호나 키가 새어 나갔을 수 있다면 데이터 키도 바꾸세요. 앞 절처럼 새 키로 백업하고 그 사본을 파일 자리에 둡니다([새 키로 백업하기](./tools.md#새-키로-백업하기)).

## 데이터베이스 복호화하기

`plain`을 준 백업은 데이터베이스를 암호화하지 않은 사본으로 쓰고, 그 사본이 암호화한 파일 자리를 차지합니다. 파일을 그 자리에서 복호화하는 호출은 없습니다.

::: lang rust

```rust
use darudb::{BackupOptions, OpenOptions};

fn decrypt(key: [u8; 32]) -> Result<(), Box<dyn std::error::Error>> {
    let db = OpenOptions::new().key(key).open("app.darudb")?;

    db.backup_with("app.plain.darudb", BackupOptions::new().plain())?;
    db.close()?;
    std::fs::rename("app.plain.darudb", "app.darudb")?;

    Ok(())
}
```

:::

::: lang node

```ts
import { renameSync } from 'node:fs';

const db = Database.open('app.darudb', { schema: app, key });

db.backup('app.plain.darudb', { plain: true }); // 또는 `await db.backupAsync(...)`
db.close();
renameSync('app.plain.darudb', 'app.darudb');
```

:::

::: lang dart

```dart
import 'dart:io';

final db = Database.open('app.darudb', schema: const Schema(1, [userSchema]), key: key);

db.backup('app.plain.darudb', plain: true); // 또는 `await db.backupAsync(...)`
db.close();
File('app.plain.darudb').renameSync('app.darudb');
```

:::

::: lang python

```python
import os

db = darudb.Database.open("app.darudb", schema=darudb.Schema(1, [User]), key=key)
db.backup("app.plain.darudb", plain=True)  # 또는 `await db.backup_async(...)`
db.close()
os.replace("app.plain.darudb", "app.darudb")
```

:::

- 사본에는 엔진 자신의 트리까지 파일의 모든 트리가 들어가므로, 자동 증가 카운터와 저장된 스키마도 그대로 넘어가고 사본을 쓰는 데 스키마가 필요 없습니다.
- [기존 데이터베이스 암호화하기](#기존-데이터베이스-암호화하기)와 마찬가지로, 사본을 옮겨 놓기 전에 쓰기를 멈추고 모든 프로세스에서 암호화한 파일의 핸들을 닫으세요.
- 사본을 옮겨 놓은 뒤에는 파일을 읽을 수 있는 사람이면 누구나 데이터를 읽고, 그 뒤에 만든 백업이나 동기화 사본을 읽을 수 있는 사람도 마찬가지입니다.

## 고려할 점

- **키를 잃으면 데이터도 잃습니다.** 키와 비밀번호를 모두 잃은 파일을 여는 호출이나 도구는 없고, 다른 길도 없습니다. 첫 커밋 전에 키를 저장하고, 운영체제 키 저장소나 비밀 관리 서비스처럼 자체 백업이 있는 곳에 두세요.
- **키를 애플리케이션 코드에 적지 마세요.** 애플리케이션의 모든 사본이 같은 키를 쓰게 되고, 사본을 가진 누구나 프로그램에서 키를 꺼낼 수 있습니다. 사용자 기기에 설치되는 앱은 [위](#운영체제-키-저장소에-키-보관하기)처럼 기기마다 키를 만들어 키 저장소에 둡니다.
- **비밀번호는 추측하기 어려워야 합니다.** Argon2id는 추측 한 번에 파일을 한 번 여는 만큼의 메모리와 시간이 들게 해서 추측을 늦출 뿐, 막지는 못합니다. 짧거나 흔한 비밀번호는 결국 뚫립니다. 비용을 올리면 추측만큼 파일 열기도 느려집니다.
- **모든 프로세스가 해시 비용을 냅니다.** 비밀번호 파일은 열 때마다 그 비용만큼 메모리를 씁니다. 기본값은 19 MiB이고, 파일을 여는 프로세스마다 듭니다. 모바일 앱 확장은 프로세스 전체가 쓸 수 있는 메모리가 이보다 조금 많은 정도일 수 있습니다.
- **페이지를 읽고 쓰는 비용이 조금 늘어납니다.** 디스크에서 읽는 페이지는 모두 복호화하고 검사하며, 쓰는 페이지는 모두 암호화합니다. 파일은 처음 만든 컴퓨터가 고른 암호 방식을 계속 쓰므로, 그 방식이 맞지 않는 프로세서에서는 더 느립니다.
- **모든 핸들이 키를 줘야 합니다.** 파일을 여는 프로세스마다, 그리고 한 프로세스 안에서 여는 핸들마다 키나 비밀번호를 줍니다. 주지 않으면 다른 핸들이 파일을 열어 둔 동안에도 `KEY_REQUIRED`로 실패합니다.
- **프로세스 안에서는 데이터가 평문입니다.** 데이터베이스가 열려 있는 동안 데이터 키와 읽은 페이지는 프로세스 메모리에 있고, 디버거나 크래시 덤프, 시스템의 스왑 파일로 닿을 수 있습니다. 키는 바이트로 넘기고, 호출이 반환되면 가지고 있던 사본을 지우세요. 문자열은 지울 수 없습니다.
- **크기와 커밋은 드러납니다.** 평문 헤더에 파일 크기와 트랜잭션 ID가 보이므로, 데이터가 얼마나 있고 얼마나 자주 바뀌는지는 키 없이도 알 수 있습니다. 파일 전체를 예전 사본으로 되돌려 놓아도 파일 안에서는 알아챌 수 없습니다. 이를 알아채야 하는 애플리케이션은 가장 최근 트랜잭션 ID를 다른 곳에 보관하며, Rust API에서는 `ReadTransaction::commit_id`로 읽습니다.

## 암호화한 파일과 도구

암호화한 파일의 백업은 같은 데이터 키로 암호화되고, 같은 키나 비밀번호로 열립니다. 다만 백업에 키나 비밀번호를 따로 주면 새 데이터 키로 암호화되고([키나 비밀번호 바꾸기](#키나-비밀번호-바꾸기)), `plain`을 주면 암호화되지 않습니다([데이터베이스 복호화하기](#데이터베이스-복호화하기)). 되살리기는 손상된 파일의 키나 비밀번호를 받고, 그 키나 비밀번호로 새 파일도 열립니다. [도구](./tools.md)를 보세요.
