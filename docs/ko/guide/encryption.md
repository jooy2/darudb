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

페이지 안의 모든 것이 암호화됩니다. 키와 값, 컬렉션과 트리의 이름도 마찬가지입니다. 모든 페이지와 헤더에 기록된 커밋 정보도 인증되므로, 바이트 하나라도 바뀌면 그대로 읽지 않고 `CORRUPTED`로 알립니다. 키 없이 열면 `KEY_REQUIRED`, 틀린 키로 열면 `WRONG_KEY`로 실패합니다.

## 키와 비밀번호

- **키**는 무작위 32바이트입니다. [아래](#운영체제-키-저장소에-키-보관하기)처럼 운영체제의 키 저장소에 보관한 키가 그런 예입니다.
- **비밀번호**는 Argon2id로 키를 만듭니다. 기본 비용은 19 MiB, 반복 2회, 병렬 1이고 수십 밀리초가 걸립니다. <LangCode rust="OpenOptions::password_hashing" node="passwordHashing" dart="passwordHashing" />으로 새 파일과 비밀번호 변경에 쓸 비용을 올리거나 내립니다. 파일은 만들 때의 비용을 기록해 두므로, 열 때는 옵션과 관계없이 그 비용이 듭니다.
- **키나 비밀번호를 바꿔도** 페이지를 다시 암호화하지 않고, 바꾸기가 끝나면 이전 것으로는 파일을 열 수 없습니다.
- 평문 데이터베이스는 평문으로 남고, 암호화한 데이터베이스는 키 없이 열 수 없습니다. 키나 비밀번호를 잃어버리지 않을 곳에 보관하세요. 잃어버리면 데이터를 읽을 방법이 없습니다.

페이지를 어떻게 암호화하고 키를 파일에 어떻게 보관하는지는 엔진 섹션의 [암호화](../engine/encryption.md)에 있습니다.

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

## 암호화한 파일과 도구

암호화한 파일의 백업은 같은 키로 암호화되고, 같은 키나 비밀번호로 열립니다. 다만 백업에 키나 비밀번호를 따로 주면 새 데이터 키로 암호화됩니다. 키나 비밀번호를 바꾸는 것으로는 데이터 키 자체가 바뀌지 않으므로, 데이터 키를 바꾸는 방법은 이것입니다([새 키로 백업하기](./tools.md#새-키로-백업하기)). 되살리기는 손상된 파일의 키나 비밀번호를 받고, 그 키나 비밀번호로 새 파일도 열립니다. [도구](./tools.md)를 보세요.
