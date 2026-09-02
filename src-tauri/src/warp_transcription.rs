use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use reqwest::{redirect::Policy, Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const FIREBASE_TOKEN_URL: &str = match option_env!("HANDY_WARP_FIREBASE_TOKEN_URL") {
    Some(value) => value,
    None => "",
};
const WARP_TRANSCRIBE_URL: &str = "https://app.warp.dev/ai/transcribe";

#[derive(Debug, Deserialize)]
struct PersistedUser {
    #[serde(default)]
    id_token: Option<PersistedIdToken>,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PersistedIdToken {
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    id_token: String,
}

#[derive(Debug, Serialize)]
struct TranscribeRequest {
    provider: &'static str,
    audio: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TranscribeResponse {
    text: String,
}

struct Endpoints<'a> {
    token: &'a str,
    transcribe: &'a str,
}

pub(crate) async fn transcribe(
    samples: &[f32],
    persisted_user_json: &str,
    language: &str,
) -> Result<String> {
    transcribe_with_endpoints(
        samples,
        persisted_user_json,
        language,
        Endpoints {
            token: FIREBASE_TOKEN_URL,
            transcribe: WARP_TRANSCRIBE_URL,
        },
    )
    .await
}

async fn transcribe_with_endpoints(
    samples: &[f32],
    persisted_user_json: &str,
    language: &str,
    endpoints: Endpoints<'_>,
) -> Result<String> {
    if samples.is_empty() {
        bail!("Warp transcription requires non-empty audio");
    }

    let refresh_token = extract_refresh_token(persisted_user_json)?;
    let client = Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(45))
        .build()
        .context("failed to initialize Warp transcription client")?;

    let refresh_response = client
        .post(endpoints.token)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ])
        .send()
        .await
        .context("could not reach Warp authentication")?;
    require_success(refresh_response.status(), "Warp authentication")?;
    let id_token = refresh_response
        .json::<RefreshResponse>()
        .await
        .context("Warp authentication returned an invalid response")?
        .id_token;
    if id_token.is_empty() {
        bail!("Warp authentication returned an empty token");
    }

    let request = TranscribeRequest {
        provider: "wispr",
        audio: BASE64.encode(encode_pcm16_wav(samples)?),
        language: normalize_language(language),
    };
    let response = client
        .post(endpoints.transcribe)
        .bearer_auth(id_token)
        .json(&request)
        .send()
        .await
        .context("could not reach Warp transcription")?;
    require_success(response.status(), "Warp transcription")?;

    let text = response
        .json::<TranscribeResponse>()
        .await
        .context("Warp transcription returned an invalid response")?
        .text;
    Ok(text)
}

fn extract_refresh_token(persisted_user_json: &str) -> Result<String> {
    if persisted_user_json.trim().is_empty() {
        bail!("Paste Warp Terminal credentials in General settings first");
    }
    let persisted: PersistedUser = serde_json::from_str(persisted_user_json)
        .map_err(|_| anyhow!("Warp credentials are not valid PersistedUser JSON"))?;
    let token = persisted
        .id_token
        .map(|token| token.refresh_token)
        .or(persisted.refresh_token)
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| anyhow!("Warp credentials do not contain a refresh token"))?;
    Ok(token)
}

fn normalize_language(language: &str) -> Option<String> {
    let language = language.trim();
    if language.is_empty() || language.eq_ignore_ascii_case("auto") {
        None
    } else {
        Some(language.to_string())
    }
}

fn require_success(status: StatusCode, operation: &str) -> Result<()> {
    if status.is_success() {
        Ok(())
    } else {
        bail!("{operation} failed with HTTP {status}")
    }
}

fn encode_pcm16_wav(samples: &[f32]) -> Result<Vec<u8>> {
    let data_len = samples
        .len()
        .checked_mul(2)
        .and_then(|len| u32::try_from(len).ok())
        .ok_or_else(|| anyhow!("audio is too large for a WAV request"))?;
    let riff_len = 36_u32
        .checked_add(data_len)
        .ok_or_else(|| anyhow!("audio is too large for a WAV request"))?;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_len.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&16_000_u32.to_le_bytes());
    wav.extend_from_slice(&32_000_u32.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        wav.extend_from_slice(&pcm.to_le_bytes());
    }
    Ok(wav)
}

#[cfg(test)]
mod tests {
    use super::{encode_pcm16_wav, extract_refresh_token, transcribe_with_endpoints, Endpoints};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn accepts_current_and_legacy_refresh_token_shapes() {
        assert_eq!(
            extract_refresh_token(r#"{"id_token":{"refresh_token":"nested"}}"#).unwrap(),
            "nested"
        );
        assert_eq!(
            extract_refresh_token(r#"{"refresh_token":"legacy"}"#).unwrap(),
            "legacy"
        );
    }

    #[test]
    fn encodes_complete_mono_16khz_pcm16_wav() {
        let wav = encode_pcm16_wav(&[0.0, 1.0, -1.0]).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
        assert_eq!(wav.len(), 50);
    }

    #[test]
    fn refreshes_credentials_then_sends_wispr_audio() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (first, mut first_stream) = read_request(&listener);
            assert!(first.starts_with("POST /token "));
            assert!(first.contains("grant_type=refresh_token"));
            assert!(first.contains("refresh_token=refresh-value"));
            write_response(&mut first_stream, r#"{"id_token":"short-lived-id"}"#);

            let (second, mut second_stream) = read_request(&listener);
            assert!(second.starts_with("POST /transcribe "));
            assert!(second.contains("authorization: Bearer short-lived-id"));
            assert!(second.contains(r#""provider":"wispr""#));
            assert!(second.contains(r#""language":"en""#));
            write_response(&mut second_stream, r#"{"text":"mock transcript"}"#);
        });

        let result = tauri::async_runtime::block_on(transcribe_with_endpoints(
            &[0.0; 160],
            r#"{"id_token":{"refresh_token":"refresh-value"}}"#,
            "en",
            Endpoints {
                token: &format!("{base}/token"),
                transcribe: &format!("{base}/transcribe"),
            },
        ))
        .unwrap();
        server.join().unwrap();
        assert_eq!(result, "mock transcript");
    }

    fn read_request(listener: &TcpListener) -> (String, std::net::TcpStream) {
        let (mut stream, _) = listener.accept().unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            bytes.extend_from_slice(&buffer[..count]);
            let Some(header_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") else {
                continue;
            };
            let header_end = header_end + 4;
            let headers = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
            let content_len = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if bytes.len() >= header_end + content_len {
                return (String::from_utf8_lossy(&bytes).to_string(), stream);
            }
        }
    }

    fn write_response(stream: &mut std::net::TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    #[test]
    #[ignore = "requires HANDY_WARP_CREDENTIALS_FILE and HANDY_WARP_AUDIO_FILE"]
    fn live_warp_transcription() {
        let credentials_path = std::env::var("HANDY_WARP_CREDENTIALS_FILE").unwrap();
        let audio_path = std::env::var("HANDY_WARP_AUDIO_FILE").unwrap();
        let credentials = std::fs::read_to_string(credentials_path).unwrap();
        let samples = crate::audio_toolkit::read_wav_samples(audio_path).unwrap();
        let started = std::time::Instant::now();
        let text = tauri::async_runtime::block_on(super::transcribe(&samples, &credentials, "en"))
            .unwrap();
        assert!(!text.trim().is_empty());
        eprintln!(
            "Warp live transcription completed in {:?}",
            started.elapsed()
        );
    }
}
