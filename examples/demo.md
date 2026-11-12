---
title: Markdown demo
author: you
tags:
  - gfm
  - demo
---

# Markdown

Лёгкий просмотрщик и редактор **Markdown** для Windows — рендер *как на GitHub*,
~~тяжёлый Electron~~ не нужен :rocket:. Ссылки работают: https://github.com и [раздел ниже](#таблицы).

> [!NOTE]
> Нажмите <kbd>Ctrl</kbd>+<kbd>E</kbd>, чтобы переключиться в **Raw**, или <kbd>Ctrl</kbd>+<kbd>3</kbd> — режим «Рядом».

> [!TIP]
> Галочки в списке задач кликабельны прямо в просмотре — исходник обновится сам.

> [!IMPORTANT]
> Файл отслеживается: если его поменять в другом редакторе, просмотр обновится.

> [!WARNING]
> Несохранённые правки помечаются точкой рядом с именем файла.

> [!CAUTION]
> Скрипты внутри документа не выполняются.

## Списки задач

- [x] Рендер GFM
- [x] Подсветка кода
- [ ] Написать README
  - [ ] вложенная задача

1. Первый
2. Второй
   1. Вложенный
   2. Ещё один

## Таблицы

| Возможность        | Статус | Комментарий            |
|:-------------------|:------:|-----------------------:|
| Таблицы            |   ✅   |      выравнивание тоже |
| Alerts             |   ✅   |  `> [!NOTE]` и другие  |
| Сноски[^1]         |   ✅   |                    да! |
| Emoji `:tada:`     |   🎉   |                        |

## Код

Встроенный `code`, а ниже — блоки:

```rust
use std::collections::HashMap;

/// Counts words in a string.
fn count(text: &str) -> HashMap<&str, usize> {
    let mut map = HashMap::new();
    for w in text.split_whitespace() {
        *map.entry(w).or_insert(0) += 1;
    }
    map
}
```

```ts
interface User { id: number; name: string }
const greet = (u: User): string => `Hello, ${u.name}!`;
```

```powershell
Get-ChildItem -Path $env:USERPROFILE -Filter *.md -Recurse |
    Where-Object { $_.Length -gt 1KB } | Select-Object Name, Length
```

```diff
- old line
+ new line
```

```json
{ "name": "markdown", "version": "1.0.0", "private": true }
```

## Прочее

<details>
<summary>Раскрывающийся блок (raw HTML)</summary>

Внутри можно писать **markdown**.

</details>

Горизонтальная линия:

---

Текст со сноской[^2] и <mark>выделением</mark>. H<sub>2</sub>O и E = mc<sup>2</sup>.

[^1]: Сноски рендерятся внизу страницы, как на GitHub.
[^2]: Вторая сноска.
