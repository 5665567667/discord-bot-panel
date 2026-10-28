#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::{io::Write, net::TcpStream, path::PathBuf, process::{Child, Command, Stdio}, sync::Mutex, thread, time::Duration};
use tauri::{Manager, State};

struct ProcessState { child: Mutex<Option<Child>>, log_file: Mutex<Option<std::fs::File>>, data_dir: PathBuf }
#[derive(serde::Serialize)] struct ResultMessage { ok: bool, message: String }
#[derive(serde::Serialize)] struct Health { api_ready: bool, message: String }
fn result(ok: bool, message: impl Into<String>) -> ResultMessage { ResultMessage { ok, message: message.into() } }
fn backend_path(app: &tauri::AppHandle, data_dir: &PathBuf) -> Result<PathBuf, String> {
    if let Ok(resource) = app.path().resolve("binaries/panel-backend.exe", tauri::path::BaseDirectory::Resource) {
        if resource.exists() { return Ok(resource); }
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    for path in [cwd.join("target/release/panel-backend.exe"), cwd.join("../target/release/panel-backend.exe"), data_dir.join("panel-backend.exe")] {
        if path.exists() { return Ok(path); }
    }
    Err("Backend-файл не найден. Выполните сборку из Start all in one.bat.".into())
}
fn check_http() -> bool { TcpStream::connect_timeout(&"127.0.0.1:8787".parse().unwrap(),Duration::from_millis(400)).is_ok() }
#[tauri::command]
fn start_backend(app: tauri::AppHandle, state: State<'_, ProcessState>) -> Result<ResultMessage, String> {
    if check_http() { return Ok(result(true,"Backend уже запущен.")); }
    let mut child_slot=state.child.lock().map_err(|e|e.to_string())?;
    if child_slot.is_some() { return Ok(result(false,"Backend запускается. Проверьте журнал, если API не появится.")); }
    let exe=backend_path(&app,&state.data_dir)?;
    let env_file=state.data_dir.join(".env");
    if !env_file.exists() { return Ok(result(false,format!("Сначала настройте файл {}",env_file.display()))); }
    let log_path=state.data_dir.join("logs/panel.log");
    std::fs::create_dir_all(log_path.parent().unwrap()).map_err(|e|e.to_string())?;
    let mut log=std::fs::OpenOptions::new().create(true).append(true).open(&log_path).map_err(|e|e.to_string())?;
    writeln!(log,"\n--- Запуск backend ---").ok();
    let output=log.try_clone().map_err(|e|e.to_string())?;
    let child=Command::new(exe).current_dir(&state.data_dir).stdin(Stdio::null()).stdout(Stdio::from(output)).stderr(Stdio::from(log.try_clone().map_err(|e|e.to_string())?)).spawn().map_err(|e|format!("Не удалось запустить backend: {e}"))?;
    *state.log_file.lock().map_err(|e|e.to_string())?=Some(log);
    *child_slot=Some(child);
    drop(child_slot);
    for _ in 0..30 { if check_http(){return Ok(result(true,"Backend запущен."));} thread::sleep(Duration::from_millis(250)); }
    Ok(result(false,"Процесс запускается, но API не ответил за 7,5 секунды. Откройте журнал."))
}
#[tauri::command]
fn stop_backend(state: State<'_, ProcessState>) -> Result<ResultMessage,String> {
    let mut child_slot=state.child.lock().map_err(|e|e.to_string())?;
    let Some(mut child)=child_slot.take() else {return Ok(result(true,"Backend не запускался через это окно."));};
    child.kill().map_err(|e|format!("Не удалось остановить backend: {e}"))?;
    let _=child.wait();
    Ok(result(true,"Backend остановлен."))
}
#[tauri::command]
fn backend_health()->Health { let ready=check_http(); Health{api_ready:ready,message:if ready{"API отвечает на 127.0.0.1:8787"}else{"API пока недоступен"}.into()} }
#[tauri::command]
fn prepare_config(state: State<'_,ProcessState>)->Result<ResultMessage,String>{
    let path=state.data_dir.join(".env");
    if path.exists(){return Ok(result(true,format!("Конфигурация: {}",path.display())))}
    let template="DISCORD_BOT_TOKEN=\nDISCORD_CLIENT_ID=\nDISCORD_CLIENT_SECRET=\nDISCORD_REDIRECT_URI=http://127.0.0.1:8787/auth/callback\nAPP_ENCRYPTION_KEY=\nBIND_ADDR=127.0.0.1:8787\nDATABASE_URL=sqlite://data/panel.db?mode=rwc\nCOOKIE_SECURE=false\n";
    std::fs::write(&path,template).map_err(|e|e.to_string())?;
    Ok(result(true,format!("Создан {}. Заполните реквизиты Discord и ключ шифрования.",path.display())))
}
fn main(){
    tauri::Builder::default()
        .setup(|app|{
            let data_dir=app.path().app_data_dir()?;
            std::fs::create_dir_all(data_dir.join("logs"))?;
            std::fs::create_dir_all(data_dir.join("data"))?;
            app.manage(ProcessState{child:Mutex::new(None),log_file:Mutex::new(None),data_dir});
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_backend,stop_backend,backend_health,prepare_config])
        .build(tauri::generate_context!())
        .expect("Could not build desktop app")
        .run(|app,event|{
            if matches!(event,tauri::RunEvent::Exit){
                if let Some(state)=app.try_state::<ProcessState>(){
                    if let Ok(mut slot)=state.child.lock(){if let Some(mut child)=slot.take(){let _=child.kill();let _=child.wait();}}
                }
            }
        });
}
