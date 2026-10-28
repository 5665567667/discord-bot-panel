# Discord Bot Panel

Локальная панель управления Discord-ботом для Windows. Backend на Rust использует Axum, Serenity, SQLx и SQLite. Интерфейс написан на React и TypeScript, а отдельное окно Windows подготавливается через Tauri.

## Текущий статус

В репозитории теперь есть рабочая основа проекта: OAuth2 Authorization Code Flow с state-проверкой, серверные сессии и CSRF-токен, шифрование OAuth токенов в SQLite, проверка доступа Discord к серверам, сохранение настроек, журнал действий, API для предупреждения, бана, кика и тайм-аута, Serenity Gateway, React UI, подтверждение модераторских действий и Windows BAT-меню.

Это ещё не завершённая реализация исходного объёмного ТЗ. Полный набор slash-команд, автоматические приветствия, команды очистки/slowmode/lock, управление ролями и загрузка/запуск VPN-профиля не реализованы. Сборка и тесты не выполнялись в этой среде. Не считайте проект production-ready до успешных CI и проверки Discord конфигурации.

## Требования

Windows 10/11 x64, Rust stable с Cargo, Node.js LTS с npm. Для сборки Tauri потребуется установленный WebView2 Runtime и C++ Build Tools для соответствующего toolchain.

## Настройка Discord

1. Создайте приложение и бота в Discord Developer Portal.
2. В OAuth2 добавьте redirect URI `http://127.0.0.1:8787/auth/callback`.
3. Запишите bot token, application client ID и client secret.
4. Скопируйте `.env.example` в `.env`.
5. Заполните `DISCORD_BOT_TOKEN`, `DISCORD_CLIENT_ID`, `DISCORD_CLIENT_SECRET` и `DISCORD_REDIRECT_URI`.
6. Сгенерируйте 32 случайных байта и запишите их hex-код в `APP_ENCRYPTION_KEY`. В PowerShell можно выполнить:

```powershell
$bytes = New-Object byte[] 32
[Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
[Convert]::ToHexString($bytes).ToLower()
```

Не публикуйте `.env`, bot token, client secret или ключ шифрования. Потеря encryption key сделает сохранённые OAuth токены нечитаемыми, тогда пользователям потребуется войти снова.

Добавьте бота на Discord-сервер с минимальными необходимыми разрешениями. Для API модерации понадобятся Ban Members, Kick Members, Moderate Members и нужные права канала. Роль бота должна стоять выше ролей, к которым применяются действия. Discord Gateway в текущем коде запрашивает только GUILDS intent.

## Запуск в режиме разработки

Из корня проекта:

```text
cd frontend
npm install
npm run dev
```

В другом окне из корня проекта запустите backend:

```text
cargo run --bin panel-backend
```

Откройте Vite URL `http://127.0.0.1:5173`. Backend слушает `127.0.0.1:8787`. Настройте `.env` перед запуском. OAuth callback должен точно совпадать с redirect URI, сохранённым в Discord Developer Portal.

## Windows-меню

Запустите `Start all in one.bat` и выберите пункт по номеру. Установка загружает npm-зависимости. Сборка запускает typecheck, сборку frontend, Rust checks, тесты, Clippy и сборку backend, затем собирает Tauri installer. Запуск открывает desktop app. Резервное копирование копирует SQLite-файл, поэтому для согласованной копии остановите backend перед созданием backup.

Скрипт не скачивает неизвестные исполняемые файлы и не запускает процессы от имени администратора. Если Rust или Node.js отсутствуют, открывается официальный сайт поставщика.

## API

Все API маршруты работают на локальном origin. `/api/status` сообщает состояние Gateway и uptime. OAuth-авторизация начинается на `/auth/login`. После входа доступны профиль, серверы, настройки сервера, журнал и moderation endpoint. Изменяющие запросы требуют CSRF cookie и заголовок `X-CSRF-Token`. Доступ к серверу каждый раз проверяется по Discord OAuth guild permissions. Discord REST ошибки возвращаются как ошибки API, а не как фиктивный успех.

## Резервное копирование

Остановите backend, затем используйте пункт меню резервного копирования. Копии хранятся в `backups`. Сохраняйте ключ `APP_ENCRYPTION_KEY` вместе с резервной копией в отдельном защищённом месте, иначе OAuth токены из копии восстановить нельзя. Не храните секреты в публичном репозитории.

## Проверки

```text
cargo fmt --check
cargo check --bin panel-backend
cargo test --bin panel-backend
cargo clippy --bin panel-backend -- -D warnings
```

В frontend:

```text
npm run typecheck
npm run build
```

Команды также включены в BAT файл. В рамках этой правки их выполнение не подтверждалось. Проверьте сборку на Windows 10/11, OAuth callback, права роли, модерацию, остановку процесса, установщик, обновление и восстановление базы.

## Сетевой раздел

Интерфейс показывает локальный bind address и ссылку на страницу релизов Flowseal. Приложение намеренно не скачивает и не запускает сторонний `service.bat`, не устанавливает службы, не меняет DNS и системные маршруты. Импорт/активация VPN-профилей пока не реализованы. Перед ручной установкой стороннего ПО проверьте источник, содержимое архива и системный запрос подтверждения.
