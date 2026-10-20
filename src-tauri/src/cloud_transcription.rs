//! Streaming transports and credentials for Cartesia and ElevenLabs.
use crate::settings::{get_settings, AppSettings, TranscriptionProvider};
use anyhow::Result;
use base64::Engine as _;
#[cfg(all(unix, any(debug_assertions, not(target_os = "macos"))))]
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};
#[cfg(any(debug_assertions, not(target_os = "macos")))]
use std::{fs, fs::OpenOptions, path::PathBuf};
use tauri::AppHandle;
#[cfg(any(debug_assertions, not(target_os = "macos")))]
use tauri::Manager;
use tungstenite::{
    client::IntoClientRequest, http::HeaderValue, stream::MaybeTlsStream, Message, WebSocket,
};
pub(crate) type CloudSocket = WebSocket<MaybeTlsStream<std::net::TcpStream>>;
#[derive(Clone, Copy, Debug)]
pub(crate) enum CloudSttProvider {
    Cartesia,
    ElevenLabs,
}

impl CloudSttProvider {
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        match id.to_ascii_lowercase().as_str() {
            "cartesia" => Some(Self::Cartesia),
            "elevenlabs" | "eleven_labs" | "eleven" => Some(Self::ElevenLabs),
            _ => None,
        }
    }

    pub(crate) fn from_settings(settings: &AppSettings) -> Option<Self> {
        match settings.transcription_provider {
            TranscriptionProvider::Local => None,
            TranscriptionProvider::WarpWispr | TranscriptionProvider::WisprFlow => None,
            TranscriptionProvider::Cartesia => Some(Self::Cartesia),
            TranscriptionProvider::ElevenLabs => Some(Self::ElevenLabs),
        }
    }

    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    fn keychain_service(self) -> &'static str {
        match self {
            Self::Cartesia => "handy-cartesia-stt",
            Self::ElevenLabs => "handy-elevenlabs-stt",
        }
    }

    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    fn keychain_api_key(self) -> Option<String> {
        const ACCOUNT: &str = "Handy";
        if let Ok(password) =
            security_framework::passwords::get_generic_password(self.keychain_service(), ACCOUNT)
        {
            return String::from_utf8(password)
                .ok()
                .filter(|value| !value.trim().is_empty());
        }

        // Compatibility with credentials created by the original test fixture,
        // whose account name predates the in-app provider selector.
        let output = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-w", "-s", self.keychain_service()])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8(output.stdout)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    #[cfg(any(debug_assertions, not(target_os = "macos")))]
    fn api_key_path(self, app: &AppHandle) -> Result<PathBuf> {
        let file_name = match self {
            Self::Cartesia => "cartesia-api-key",
            Self::ElevenLabs => "elevenlabs-api-key",
        };
        Ok(app.path().app_data_dir()?.join(file_name))
    }

    pub(crate) fn api_key(self, app: &AppHandle) -> Result<String> {
        #[cfg(all(target_os = "macos", not(debug_assertions)))]
        let _ = app;
        #[cfg(all(target_os = "macos", not(debug_assertions)))]
        if let Some(value) = self.keychain_api_key() {
            return Ok(value);
        }

        #[cfg(any(debug_assertions, not(target_os = "macos")))]
        if let Ok(value) = fs::read_to_string(self.api_key_path(app)?) {
            if !value.trim().is_empty() {
                return Ok(value.trim().to_string());
            }
        }

        Err(anyhow::anyhow!(
            "No credential is configured for this cloud transcription provider"
        ))
    }

    pub(crate) fn save_api_key(self, app: &AppHandle, api_key: &str) -> Result<()> {
        if api_key.trim().is_empty() {
            return Err(anyhow::anyhow!("API key cannot be empty"));
        }

        #[cfg(all(target_os = "macos", not(debug_assertions)))]
        {
            let _ = app;
            const ACCOUNT: &str = "Handy";
            security_framework::passwords::set_generic_password(
                self.keychain_service(),
                ACCOUNT,
                api_key.trim().as_bytes(),
            )?;
        }

        #[cfg(any(debug_assertions, not(target_os = "macos")))]
        {
            let path = self.api_key_path(app)?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut options = OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(path)?;
            std::io::Write::write_all(&mut file, api_key.trim().as_bytes())?;
        }

        Ok(())
    }
}

pub(crate) fn set_cloud_read_timeout(
    socket: &mut CloudSocket,
    timeout: Duration,
) -> std::io::Result<()> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout)),
        MaybeTlsStream::NativeTls(stream) => stream.get_mut().set_read_timeout(Some(timeout)),
        _ => Ok(()),
    }
}

fn cloud_url(provider: CloudSttProvider, settings: &AppSettings) -> Result<url::Url> {
    let language = settings.selected_language.as_str();
    let mut url = match provider {
        CloudSttProvider::Cartesia => url::Url::parse("wss://api.cartesia.ai/stt/websocket")?,
        CloudSttProvider::ElevenLabs => {
            url::Url::parse("wss://api.elevenlabs.io/v1/speech-to-text/realtime")?
        }
    };
    {
        let mut query = url.query_pairs_mut();
        match provider {
            CloudSttProvider::Cartesia => {
                query
                    .append_pair("model", "ink-2")
                    .append_pair("encoding", "pcm_s16le")
                    .append_pair("sample_rate", "16000")
                    .append_pair("cartesia_version", "2026-08-14");
                for term in &settings.custom_words {
                    query.append_pair("keyterm", term);
                }
            }
            CloudSttProvider::ElevenLabs => {
                query
                    .append_pair("model_id", "scribe_v2_realtime")
                    .append_pair("audio_format", "pcm_16000")
                    .append_pair("commit_strategy", "manual");
                if language != "auto" {
                    query.append_pair("language_code", language);
                }
                for term in settings
                    .custom_words
                    .iter()
                    .filter(|term| term.chars().count() <= 20)
                {
                    query.append_pair("keyterms", term);
                }
            }
        }
    }
    Ok(url)
}

fn pcm16_bytes(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub(crate) fn append_segment(target: &mut String, segment: &str, concatenate_exactly: bool) {
    if !concatenate_exactly && !target.is_empty() && !target.ends_with(char::is_whitespace) {
        target.push(' ');
    }
    target.push_str(segment);
}

pub(crate) fn cloud_send_audio(
    socket: &mut CloudSocket,
    provider: CloudSttProvider,
    pcm: &[f32],
) -> Result<()> {
    let bytes = pcm16_bytes(pcm);
    let message = match provider {
        CloudSttProvider::Cartesia => Message::Binary(bytes.into()),
        CloudSttProvider::ElevenLabs => Message::Text(
            serde_json::json!({
                "message_type": "input_audio_chunk",
                "audio_base_64": base64::engine::general_purpose::STANDARD.encode(bytes),
            })
            .to_string()
            .into(),
        ),
    };
    socket.send(message)?;
    Ok(())
}

pub(crate) fn eleven_commit(socket: &mut CloudSocket) -> Result<()> {
    socket.send(Message::Text(
        serde_json::json!({
            "message_type": "input_audio_chunk",
            "audio_base_64": "",
            "commit": true,
        })
        .to_string()
        .into(),
    ))?;
    Ok(())
}

pub(crate) fn connect_cloud_stt(
    provider: CloudSttProvider,
    settings: &AppSettings,
    api_key: &str,
) -> Result<CloudSocket> {
    let url = cloud_url(provider, settings)?;
    let mut request = url.as_str().into_client_request()?;
    let (header, value) = match provider {
        CloudSttProvider::Cartesia => ("X-API-Key", api_key),
        CloudSttProvider::ElevenLabs => ("xi-api-key", api_key),
    };
    request
        .headers_mut()
        .insert(header, HeaderValue::from_str(value)?);
    use std::net::{TcpStream, ToSocketAddrs};
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing cloud host"))?;
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut tcp = None;
    for address in (host, 443).to_socket_addrs()? {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        if let Ok(stream) = TcpStream::connect_timeout(&address, remaining) {
            tcp = Some(stream);
            break;
        }
    }
    let tcp =
        tcp.ok_or_else(|| anyhow::anyhow!("Could not connect to the transcription provider"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(8)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(8)))?;
    let (socket, _) = tungstenite::client_tls(request, tcp)?;
    Ok(socket)
}

pub(crate) fn cloud_event_error(event: &serde_json::Value) -> Option<String> {
    let event_type = event
        .get("type")
        .or_else(|| event.get("message_type"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if !matches!(
        event_type,
        "error"
            | "auth_error"
            | "quota_exceeded"
            | "transcriber_error"
            | "input_error"
            | "invalid_request"
            | "unaccepted_terms"
            | "rate_limited"
            | "queue_overflow"
            | "resource_exhausted"
            | "session_time_limit_exceeded"
            | "chunk_size_exceeded"
            | "commit_throttled"
    ) && !event_type.ends_with("_error")
    {
        return None;
    }

    event
        .get("message")
        .and_then(|value| value.as_str())
        .or_else(|| {
            event
                .pointer("/error/message")
                .and_then(|value| value.as_str())
        })
        .or_else(|| event.get("error").and_then(|value| value.as_str()))
        .map(str::to_owned)
        .or_else(|| Some(format!("Cloud transcription returned {event_type}")))
}

fn verification_tone() -> Vec<f32> {
    (0..16_000)
        .map(|index| {
            let phase = index as f32 * 440.0 * std::f32::consts::TAU / 16_000.0;
            phase.sin() * 0.02
        })
        .collect()
}

pub(crate) fn verify_cloud_stt_api_key(
    app: &AppHandle,
    provider: CloudSttProvider,
    candidate_api_key: Option<&str>,
) -> Result<()> {
    let api_key = match candidate_api_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        Some(key) => key.to_owned(),
        None => provider.api_key(app)?,
    };
    let settings = get_settings(app);
    let mut socket = connect_cloud_stt(provider, &settings, &api_key)?;
    verify_socket(&mut socket, provider, Duration::from_secs(12))
}

fn verify_socket(
    socket: &mut CloudSocket,
    provider: CloudSttProvider,
    timeout: Duration,
) -> Result<()> {
    set_cloud_read_timeout(socket, Duration::from_millis(50))?;

    // Run a small request through the exact model and transport used for real
    // recordings. Some providers report authorization, quota, or model errors
    // as the first WebSocket event, after the handshake has succeeded.
    cloud_send_audio(socket, provider, &verification_tone())?;
    match provider {
        CloudSttProvider::Cartesia => socket.send(Message::Text("finalize".into()))?,
        CloudSttProvider::ElevenLabs => eleven_commit(socket)?,
    }

    let deadline = Instant::now() + timeout;
    let result = loop {
        if Instant::now() >= deadline {
            break Err(anyhow::anyhow!(
                "The provider accepted the connection but did not complete the transcription model check"
            ));
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                let event: serde_json::Value = serde_json::from_str(text.as_str())?;
                if let Some(detail) = cloud_event_error(&event) {
                    break Err(anyhow::anyhow!(detail));
                }
                let event_type = event
                    .get("type")
                    .or_else(|| event.get("message_type"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default();
                let completed = match provider {
                    CloudSttProvider::Cartesia => {
                        matches!(event_type, "flush_done" | "done")
                    }
                    CloudSttProvider::ElevenLabs => matches!(
                        event_type,
                        "committed_transcript" | "committed_transcript_with_timestamps"
                    ),
                };
                if completed {
                    break Ok(());
                }
            }
            Ok(Message::Close(_)) => {
                break Err(anyhow::anyhow!(
                    "The provider closed the connection before completing the transcription model check"
                ));
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => break Err(error.into()),
        }
    };
    let _ = socket.close(None);
    result
}

/// Verify the candidate against the transcription model before replacing the saved key.
#[tauri::command]
#[specta::specta]
pub async fn save_cloud_stt_api_key(
    app: AppHandle,
    provider: String,
    api_key: String,
) -> std::result::Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider =
            CloudSttProvider::from_id(&provider).ok_or("Unknown cloud transcription provider")?;
        if api_key.trim().is_empty() {
            return Err("API key cannot be empty".to_string());
        }
        verify_before_save(
            || verify_cloud_stt_api_key(&app, provider, Some(&api_key)),
            || provider.save_api_key(&app, &api_key),
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn check_cloud_stt_api_key(
    app: AppHandle,
    provider: String,
) -> std::result::Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider =
            CloudSttProvider::from_id(&provider).ok_or("Unknown cloud transcription provider")?;
        verify_cloud_stt_api_key(&app, provider, None).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn verify_before_save(
    verify: impl FnOnce() -> Result<()>,
    save: impl FnOnce() -> Result<()>,
) -> Result<()> {
    verify()?;
    save()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn mock_provider(
        events: Vec<serde_json::Value>,
        delay: Duration,
        provider: CloudSttProvider,
    ) -> (CloudSocket, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut socket = tungstenite::accept(tcp).unwrap();
            let audio = socket.read().unwrap();
            let finalize = socket.read().unwrap();
            match provider {
                CloudSttProvider::Cartesia => {
                    assert_eq!(audio.into_data().len(), 32_000);
                    assert_eq!(finalize.into_text().unwrap(), "finalize");
                }
                CloudSttProvider::ElevenLabs => {
                    let audio: serde_json::Value =
                        serde_json::from_str(audio.to_text().unwrap()).unwrap();
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(audio["audio_base_64"].as_str().unwrap())
                        .unwrap();
                    assert_eq!(bytes.len(), 32_000);
                    let finalize: serde_json::Value =
                        serde_json::from_str(finalize.to_text().unwrap()).unwrap();
                    assert_eq!(finalize["commit"], true);
                }
            }
            thread::sleep(delay);
            for event in events {
                if socket
                    .send(Message::Text(event.to_string().into()))
                    .is_err()
                {
                    break;
                }
            }
        });
        let tcp = TcpStream::connect(address).unwrap();
        let (socket, _) =
            tungstenite::client(format!("ws://{address}/"), MaybeTlsStream::Plain(tcp)).unwrap();
        (socket, server)
    }

    #[test]
    fn verification_requires_model_completion_for_both_services() {
        for (provider, event) in [
            (
                CloudSttProvider::Cartesia,
                serde_json::json!({"type":"flush_done"}),
            ),
            (
                CloudSttProvider::ElevenLabs,
                serde_json::json!({"message_type":"committed_transcript", "text":""}),
            ),
        ] {
            let (mut socket, server) = mock_provider(vec![event], Duration::ZERO, provider);
            verify_socket(&mut socket, provider, Duration::from_secs(1)).unwrap();
            server.join().unwrap();
        }
    }

    #[test]
    fn rejection_after_handshake_preserves_saved_key() {
        let provider = CloudSttProvider::Cartesia;
        let (mut socket, server) = mock_provider(
            vec![
                serde_json::json!({"type":"transcript", "text":"", "is_final":true}),
                serde_json::json!({"type":"error", "message":"Quota exceeded"}),
            ],
            Duration::ZERO,
            provider,
        );
        let saved = std::cell::RefCell::new("previous valid key".to_string());
        let result = verify_before_save(
            || verify_socket(&mut socket, provider, Duration::from_secs(1)),
            || {
                *saved.borrow_mut() = "replacement".to_string();
                Ok(())
            },
        );
        assert!(result.unwrap_err().to_string().contains("Quota exceeded"));
        assert_eq!(&*saved.borrow(), "previous valid key");
        server.join().unwrap();
    }

    #[test]
    fn elevenlabs_model_rejection_after_handshake_does_not_save() {
        let provider = CloudSttProvider::ElevenLabs;
        let (mut socket, server) = mock_provider(
            vec![
                serde_json::json!({"message_type":"auth_error", "error":"Speech-to-text permission required"}),
            ],
            Duration::ZERO,
            provider,
        );
        let saved = std::cell::Cell::new(false);
        let error = verify_before_save(
            || verify_socket(&mut socket, provider, Duration::from_secs(1)),
            || {
                saved.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("permission required"));
        assert!(!saved.get());
        server.join().unwrap();
    }

    #[test]
    fn successful_verification_allows_replacement() {
        let provider = CloudSttProvider::Cartesia;
        let (mut socket, server) = mock_provider(
            vec![serde_json::json!({"type":"flush_done"})],
            Duration::ZERO,
            provider,
        );
        let saved = std::cell::Cell::new(false);
        verify_before_save(
            || verify_socket(&mut socket, provider, Duration::from_secs(1)),
            || {
                saved.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(saved.get());
        server.join().unwrap();
    }

    #[test]
    fn missing_completion_is_a_bounded_failure() {
        let provider = CloudSttProvider::ElevenLabs;
        let (mut socket, server) = mock_provider(vec![], Duration::from_millis(180), provider);
        let started = Instant::now();
        let error = verify_socket(&mut socket, provider, Duration::from_millis(60)).unwrap_err();
        assert!(error.to_string().contains("did not complete"));
        assert!(started.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }

    #[test]
    fn auto_language_is_not_forced_to_english_and_warp_still_deserializes() {
        let mut settings = crate::settings::get_default_settings();
        settings.selected_language = "auto".into();
        for provider in [CloudSttProvider::Cartesia, CloudSttProvider::ElevenLabs] {
            let url = cloud_url(provider, &settings).unwrap();
            assert!(!url
                .query_pairs()
                .any(|(key, _)| key == "language" || key == "language_code"));
        }
        let warp: TranscriptionProvider = serde_json::from_str("\"warp_wispr\"").unwrap();
        assert_eq!(warp, TranscriptionProvider::WarpWispr);
        settings.selected_language = "fr".into();
        assert!(cloud_url(CloudSttProvider::ElevenLabs, &settings)
            .unwrap()
            .query_pairs()
            .any(|(key, value)| key == "language_code" && value == "fr"));
    }
}
