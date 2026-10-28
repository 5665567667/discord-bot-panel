@echo off
chcp 65001 >nul
setlocal EnableExtensions
cd /d "%~dp0"
if not exist "logs" mkdir "logs"
if not exist "data" mkdir "data"
if not exist "backups" mkdir "backups"
if not exist "frontend\dist" mkdir "frontend\dist"

:menu
cls
echo ==============================================
echo        Discord Bot Panel - управление
 echo ==============================================
echo 1. Установить зависимости
echo 2. Собрать приложение и Windows-установщик
echo 3. Запустить настольное приложение
echo 4. Остановить backend
echo 5. Проверить API
echo 6. Открыть веб-панель
echo 7. Просмотреть журнал
echo 8. Создать резервную копию SQLite
echo 9. Выход
echo.
set "choice="
set /p "choice=Введите номер: "
if "%choice%"=="1" goto install
if "%choice%"=="2" goto build
if "%choice%"=="3" goto run
if "%choice%"=="4" goto stop
if "%choice%"=="5" goto health
if "%choice%"=="6" goto open
if "%choice%"=="7" goto logs
if "%choice%"=="8" goto backup
if "%choice%"=="9" exit /b 0
echo Неверный пункт.
pause
goto menu

:install
where cargo >nul 2>nul
if errorlevel 1 (echo Rust/Cargo не найдены. Открываю официальный сайт.& start "" "https://www.rust-lang.org/tools/install"& pause& goto menu)
where node >nul 2>nul
if errorlevel 1 (echo Node.js не найден. Открываю официальный сайт.& start "" "https://nodejs.org/"& pause& goto menu)
where npm >nul 2>nul
if errorlevel 1 (echo npm не найден. Проверьте установку Node.js.& pause& goto menu)
echo Rust:
rustc --version
cargo --version
echo Node:
node --version
npm --version
pushd frontend
call npm install
if errorlevel 1 (popd& goto error)
popd
if not exist ".env" if exist ".env.example" copy /Y ".env.example" ".env" >nul
if not exist ".env" (echo Создайте .env из .env.example.& pause& goto menu)
echo Зависимости установлены. Заполните .env перед запуском.
pause
goto menu

:build
where cargo >nul 2>nul
if errorlevel 1 (echo Cargo не найден.& pause& goto menu)
where npm >nul 2>nul
if errorlevel 1 (echo npm не найден.& pause& goto menu)
pushd frontend
call npm run typecheck
if errorlevel 1 (popd& goto error)
call npm run build
if errorlevel 1 (popd& goto error)
popd
cargo fmt --check
if errorlevel 1 goto error
cargo check --bin panel-backend
if errorlevel 1 goto error
cargo test --bin panel-backend
if errorlevel 1 goto error
cargo clippy --bin panel-backend -- -D warnings
if errorlevel 1 goto error
cargo build --release --bin panel-backend
if errorlevel 1 goto error
if not exist "desktop\binaries" mkdir "desktop\binaries"
copy /Y "target\release\panel-backend.exe" "desktop\binaries\panel-backend.exe" >nul
if errorlevel 1 goto error
pushd desktop
cargo tauri build
if errorlevel 1 (popd& goto error)
popd
echo Сборка завершена. Установщик ищите в desktop\target\release\bundle.
pause
goto menu

:run
if not exist "desktop\target\release\discord-bot-panel-desktop.exe" (
 echo Настольное приложение не собрано. Сначала выберите пункт 2.
 pause
 goto menu
)
start "" "%CD%\desktop\target\release\discord-bot-panel-desktop.exe"
goto menu

:stop
powershell -NoProfile -ExecutionPolicy Bypass -Command "$p=Get-CimInstance Win32_Process | Where-Object {$_.Name -eq 'panel-backend.exe' -and $_.ExecutablePath -like '%CD%*'}; if($p){$p|ForEach-Object{Stop-Process -Id $_.ProcessId -ErrorAction SilentlyContinue};'Остановка отправлена.'}else{'Backend не найден.'}"
pause
goto menu

:health
powershell -NoProfile -Command "try {$r=Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:8787/api/status' -TimeoutSec 3; $r.Content} catch {'API не отвечает на 127.0.0.1:8787.'}"
pause
goto menu

:open
start "" "http://127.0.0.1:8787"
goto menu

:logs
if exist "logs\panel.log" (start "" notepad.exe "%CD%\logs\panel.log") else (echo Журнал не создан.& pause)
goto menu

:backup
if not exist "data\panel.db" (echo База данных пока не найдена.& pause& goto menu)
for /f %%T in ('powershell -NoProfile -Command "Get-Date -Format yyyyMMdd_HHmmss"') do set "STAMP=%%T"
copy /Y "data\panel.db" "backups\panel_%STAMP%.db" >nul
if errorlevel 1 goto error
echo Резервная копия создана: backups\panel_%STAMP%.db
pause
goto menu

:error
echo Операция завершилась ошибкой. Прочитайте вывод выше.
pause
goto menu
