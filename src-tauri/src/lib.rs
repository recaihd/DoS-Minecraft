// src-tauri/src/lib.rs

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration, Instant};

#[derive(Clone, Serialize)]
struct LogEvent {
    text: String,
    #[serde(rename = "type")]
    log_type: String,
}

struct AppState {
    running: Arc<AtomicBool>,
}

#[derive(Deserialize)]
struct StartArgs {
    ip: String,
    port: u16,
    bots: u32,
    delay_ms: Option<u64>,
    stay_seconds: Option<u64>,
}


fn write_varint(value: i32, buf: &mut Vec<u8>) {
    let mut val = value as u32;
    loop {
        if (val & !0x7F) == 0 {
            buf.push(val as u8);
            return;
        }
        buf.push(((val & 0x7F) | 0x80) as u8);
        val >>= 7;
    }
}

fn read_varint(data: &[u8]) -> Option<(i32, usize)> {
    let mut result = 0i32;
    let mut shift = 0;
    for (i, &byte) in data.iter().enumerate() {
        result |= ((byte & 0x7F) as i32) << shift;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
        if shift >= 35 {
            return None;
        }
    }
    None
}

fn write_string(s: &str, buf: &mut Vec<u8>) {
    let bytes = s.as_bytes();
    write_varint(bytes.len() as i32, buf);
    buf.extend_from_slice(bytes);
}

fn write_unsigned_short(value: u16, buf: &mut Vec<u8>) {
    buf.extend_from_slice(&value.to_be_bytes());
}

fn create_handshake_packet(address: &str, port: u16) -> Vec<u8> {
    let mut data = Vec::new();
    write_varint(0x00, &mut data);
    write_varint(340, &mut data); // 1.12.2
    write_string(address, &mut data);
    write_unsigned_short(port, &mut data);
    write_varint(2, &mut data); // Login

    let mut packet = Vec::new();
    write_varint(data.len() as i32, &mut packet);
    packet.extend_from_slice(&data);
    packet
}

fn create_login_start_packet(username: &str) -> Vec<u8> {
    let mut data = Vec::new();
    write_varint(0x00, &mut data);
    write_string(username, &mut data);

    let mut packet = Vec::new();
    write_varint(data.len() as i32, &mut packet);
    packet.extend_from_slice(&data);
    packet
}

/// Keep Alive response para 1.12.2 (ID = Long)
fn create_keep_alive_response(id: i64) -> Vec<u8> {
    let mut data = Vec::new();
    write_varint(0x0B, &mut data); 
    data.extend_from_slice(&id.to_be_bytes()); 

    let mut packet = Vec::new();
    write_varint(data.len() as i32, &mut packet);
    packet.extend_from_slice(&data);
    packet
}


async fn run_bot(
    ip: String,
    port: u16,
    username: String,
    stay_seconds: u64,
    running: Arc<AtomicBool>,
    app: AppHandle,
) -> Result<(), String> {
    let addr = format!("{}:{}", ip, port);

    let mut stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("TCP: {}", e))?;

    let _ = stream.set_nodelay(true);

    stream
        .write_all(&create_handshake_packet(&ip, port))
        .await
        .map_err(|e| format!("Handshake: {}", e))?;
    stream
        .write_all(&create_login_start_packet(&username))
        .await
        .map_err(|e| format!("Login: {}", e))?;

    let _ = app.emit(
        "log",
        LogEvent {
            text: format!("{} -> ENTROU", username),
            log_type: "bot".into(),
        },
    );

    let end = Instant::now() + Duration::from_secs(stay_seconds);
    let mut buffer: Vec<u8> = Vec::with_capacity(16384);
    let mut temp = [0u8; 8192];

    while Instant::now() < end {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        match tokio::time::timeout(Duration::from_millis(400), stream.read(&mut temp)).await {
            Ok(Ok(0)) => break, 
            Ok(Ok(n)) => {
                buffer.extend_from_slice(&temp[..n]);

                
                loop {
                    if buffer.is_empty() {
                        break;
                    }

                    let Some((packet_len, len_size)) = read_varint(&buffer) else {
                        break;
                    };

                    let total = len_size + packet_len as usize;
                    if buffer.len() < total {
                        break; 
                    }

                    let packet = &buffer[len_size..total];

                    if let Some((packet_id, id_size)) = read_varint(packet) {
                        
                        if packet_id == 0x1F && packet.len() >= id_size + 8 {
                            
                            let id_bytes: [u8; 8] = packet[id_size..id_size + 8]
                                .try_into()
                                .unwrap_or([0; 8]);
                            let keep_id = i64::from_be_bytes(id_bytes);

                            let response = create_keep_alive_response(keep_id);
                            if stream.write_all(&response).await.is_err() {
                                break;
                            }
                        }
                    }

                    buffer.drain(..total);
                }
            }
            Ok(Err(_)) => break,
            Err(_) => {
               
            }
        }
    }

    let _ = stream.shutdown().await;

    let _ = app.emit(
        "log",
        LogEvent {
            text: format!("{} -> SAIU", username),
            log_type: "bot".into(),
        },
    );

    Ok(())
}


#[tauri::command]
async fn start_connections(
    app: AppHandle,
    state: State<'_, AppState>,
    args: StartArgs,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err("Já está em execução".into());
    }

    let running = state.running.clone();
    let delay = args.delay_ms.unwrap_or(120);
    let stay_seconds = args.stay_seconds.unwrap_or(120);
    let ip = args.ip.clone();
    let port = args.port;

    let _ = app.emit(
        "log",
        LogEvent {
            text: format!("Target: {}:{}", ip, port),
            log_type: "info".into(),
        },
    );
    let _ = app.emit(
        "log",
        LogEvent {
            text: format!(
                "Bots: {} | Tempo: {}s | Modo: PARALELO + KeepAlive",
                args.bots, stay_seconds
            ),
            log_type: "info".into(),
        },
    );

    let success = Arc::new(AtomicU32::new(0));
    let failed = Arc::new(AtomicU32::new(0));
    let mut handles = Vec::new();

    for i in 1..=args.bots {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        let username = format!("Bot_{}", i);
        let ip = ip.clone();
        let running = running.clone();
        let app = app.clone();
        let success = success.clone();
        let failed = failed.clone();

        let handle = tokio::spawn(async move {
            match run_bot(ip, port, username.clone(), stay_seconds, running, app.clone()).await {
                Ok(_) => {
                    success.fetch_add(1, Ordering::SeqCst);
                }
                Err(e) => {
                    failed.fetch_add(1, Ordering::SeqCst);
                    let _ = app.emit(
                        "log",
                        LogEvent {
                            text: format!("{} -> FAILED ({})", username, e),
                            log_type: "error".into(),
                        },
                    );
                }
            }
        });

        handles.push(handle);

        if delay > 0 {
            sleep(Duration::from_millis(delay)).await;
        }
    }

    for handle in handles {
        let _ = handle.await;
    }

    running.store(false, Ordering::SeqCst);

    let _ = app.emit(
        "log",
        LogEvent {
            text: format!(
                "Finalizado. Sucesso: {} | Falhas: {}",
                success.load(Ordering::SeqCst),
                failed.load(Ordering::SeqCst)
            ),
            log_type: "success".into(),
        },
    );
    let _ = app.emit("finished", ());

    Ok(())
}

#[tauri::command]
fn stop_connections(state: State<'_, AppState>) {
    state.running.store(false, Ordering::SeqCst);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            running: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            start_connections,
            stop_connections
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}