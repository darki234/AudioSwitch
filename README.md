<div align="center">

# 🔊 AudioSwitch

**Переключение аудиовыхода Windows одной клавишей**

![Rust](https://img.shields.io/badge/Rust-2021-dea584?logo=rust&logoColor=white)
![Windows](https://img.shields.io/badge/Windows-10%2F11-0078d4?logo=windows&logoColor=white)
![License](https://img.shields.io/badge/license-MIT-blue)

</div>

---

Маленькая фоновая утилита: нажимаете **Num 1**, и звук уходит на следующее устройство (например, с динамиков ноутбука на наушники). В правом нижнем углу на секунду появляется плавающая карточка с названием устройства, оформленная в стиле «острова» [Rust Browser](https://github.com/darki234/rust-browser).

## ✨ Особенности

- работает в фоне, один экземпляр, почти не потребляет ресурсов
- карточка с плавным появлением, светлой и тёмной темой по настройкам Windows и поддержкой масштабирования экрана
- список устройств настраивается файлом `devices.txt`, лишние (например, Steam Streaming) можно исключить
- один исполняемый файл без установки

## 🚀 Сборка и запуск

Нужен [Rust](https://rustup.rs) (stable).

```powershell
cargo build --release
```

Файл: `target/release/AudioSwitch.exe`. Запустите его, дальше нажимайте **Num 1**. Чтобы утилита стартовала вместе с Windows, положите ярлык в `shell:startup`.

## 🧰 Команды

| Команда | Что делает |
|---|---|
| `AudioSwitch.exe` | работать в фоне, Num 1 переключает устройство |
| `AudioSwitch.exe once` | переключить один раз и выйти |
| `AudioSwitch.exe list` | записать активные устройства в `list.txt` (`*` отмечает текущее) |
| `AudioSwitch.exe demo` | показать карточку без переключения звука |

## ⚙️ devices.txt

Файл лежит рядом с `AudioSwitch.exe`. Одна строка: часть названия устройства. `!` в начале исключает устройство, `#` начинает комментарий, пустой файл означает «все устройства».

```text
# переключать всё, кроме Steam
!Steam Streaming
```

## 📄 Лицензия

[MIT](LICENSE)
