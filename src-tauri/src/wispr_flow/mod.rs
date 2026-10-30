//! Wispr Flow desktop account login and native bidirectional gRPC transcription.
mod client_config;
mod languages;
mod protocol;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use uuid::Uuid;

const METHOD: &str = "/flow_api.v1.TranscriptionService/TranscribeStream";
const SESSION_FILE: &str = "wispr-flow-session.json";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

// Deliberately lacks Debug: session values must never enter diagnostics.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Credential {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    provider: String,
}

#[derive(Deserialize)]
struct DeviceLogin {
    status: String,
    #[serde(flatten)]
    credential: Option<Credential>,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    exp: u64,
}

impl Credential {
    fn claims(&self) -> Result<Claims> {
        // Claims supply metadata and expiry hints. The service verifies the JWT.
        let payload = self
            .access_token
            .split('.')
            .nth(1)
            .context("Invalid Wispr Flow session. Sign in again.")?;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .map_err(|_| anyhow!("Invalid Wispr Flow session. Sign in again."))?;
        let claims: Claims = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("Invalid Wispr Flow session. Sign in again."))?;
        Uuid::parse_str(&claims.sub)
            .map_err(|_| anyhow!("Invalid Wispr Flow account identifier."))?;
        Ok(claims)
    }
}

#[cfg(any(debug_assertions, not(target_os = "macos")))]
fn session_path(app: &AppHandle) -> Result<std::path::PathBuf> {
    Ok(crate::portable::app_data_dir(app)?.join(SESSION_FILE))
}

pub(crate) fn load_credential(app: &AppHandle) -> Result<Credential> {
    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    let bytes = security_framework::passwords::get_generic_password("handy-wispr-flow", "Handy")
        .map_err(|_| anyhow!("Sign in to Wispr Flow in transcription settings."))?;
    #[cfg(any(debug_assertions, not(target_os = "macos")))]
    let bytes = {
        let path = session_path(app)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&path)
                .map(|m| m.permissions().mode() & 0o077 != 0)
                .unwrap_or(false)
            {
                bail!("Wispr Flow session file must be readable only by its owner.");
            }
        }
        std::fs::read(path)
            .map_err(|_| anyhow!("Sign in to Wispr Flow in transcription settings."))?
    };
    let credential: Credential = serde_json::from_slice(&bytes)
        .map_err(|_| anyhow!("Invalid saved Wispr Flow session. Sign in again."))?;
    credential.claims()?;
    Ok(credential)
}

fn save_credential(app: &AppHandle, credential: &Credential) -> Result<()> {
    let bytes = serde_json::to_vec(credential)?;
    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    security_framework::passwords::set_generic_password("handy-wispr-flow", "Handy", &bytes)
        .map_err(|_| anyhow!("Could not save the Wispr Flow session in Keychain."))?;
    #[cfg(any(debug_assertions, not(target_os = "macos")))]
    {
        use std::io::Write;
        let path = session_path(app)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(path)
            .context("Could not save the Wispr Flow session.")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&bytes)?;
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()?)
}

pub(crate) async fn credential(app: &AppHandle) -> Result<Credential> {
    let mut saved = load_credential(app)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    if saved.claims()?.exp > now + 60 {
        return Ok(saved);
    }
    if saved.refresh_token.is_empty()
        || (!saved.provider.is_empty() && saved.provider != "supabase")
    {
        bail!("Wispr Flow session expired. Sign in again in transcription settings.");
    }
    let response = http_client()?
        .post(client_config::AUTH_URL)
        .header("apikey", client_config::AUTH_ANON_KEY)
        .json(&serde_json::json!({"refresh_token": saved.refresh_token}))
        .send()
        .await
        .map_err(|_| anyhow!("Could not refresh the Wispr Flow session."))?;
    if !response.status().is_success() {
        bail!("Wispr Flow session expired. Sign in again in transcription settings.");
    }
    let tokens: Credential = response
        .json()
        .await
        .map_err(|_| anyhow!("Invalid Wispr Flow refresh response."))?;
    tokens.claims()?;
    saved.access_token = tokens.access_token;
    saved.refresh_token = tokens.refresh_token;
    save_credential(app, &saved)?;
    Ok(saved)
}

#[tauri::command]
#[specta::specta]
pub fn wispr_flow_session_status(app: AppHandle) -> bool {
    load_credential(&app).is_ok()
}

#[tauri::command]
#[specta::specta]
pub async fn sign_in_wispr_flow(app: AppHandle) -> std::result::Result<(), String> {
    async fn login(app: &AppHandle) -> Result<()> {
        let code = Uuid::new_v4().to_string();
        let mut url = reqwest::Url::parse("https://wisprflow.ai/login")?;
        url.query_pairs_mut()
            .append_pair("device_code", &code)
            .append_pair("auto_signin", "false")
            .append_pair("skip_default_trial", "true");
        app.opener()
            .open_url(url.as_str(), None::<&str>)
            .map_err(|_| anyhow!("Could not open the Wispr Flow sign-in page."))?;
        let client = http_client()?;
        let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
        while tokio::time::Instant::now() < deadline {
            let response = client
                .get("https://api.wisprflow.ai/device/poll")
                .query(&[("device_code", &code)])
                .send()
                .await;
            if let Ok(response) = response {
                if response.status().is_success() {
                    if let Ok(result) = response.json::<DeviceLogin>().await {
                        if result.status == "complete" {
                            let saved = result
                                .credential
                                .context("Wispr Flow sign-in returned no session.")?;
                            saved.claims()?;
                            verify_credential(&saved).await?;
                            save_credential(app, &saved)?;
                            return Ok(());
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        bail!("Wispr Flow sign-in timed out. Try signing in again.")
    }
    login(&app).await.map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn check_wispr_flow_session(app: AppHandle) -> std::result::Result<(), String> {
    async fn check(app: &AppHandle) -> Result<()> {
        verify_credential(&credential(app).await?).await
    }
    check(&app).await.map_err(|e| e.to_string())
}

fn language_codes(language: &str) -> Result<Vec<i32>> {
    let language = match language {
        "auto" => return Ok(Vec::new()),
        "zh-Hans" => "zhcn",
        "zh-Hant" => "zh",
        "nb" => "no",
        other => other,
    };
    languages::LANGUAGES
        .iter()
        .find(|(code, _)| *code == language)
        .map(|(_, value)| vec![*value])
        .context("This language is unavailable in Wispr Flow. Choose another language or Auto.")
}

fn init_request(
    credential: &Credential,
    language: &str,
    words: &[String],
) -> Result<protocol::Request> {
    Ok(protocol::Request {
        init: Some(protocol::Init {
            metadata: Some(protocol::Metadata {
                user_id: credential.claims()?.sub,
                session_id: Uuid::new_v4().to_string(),
                request_id: Uuid::new_v4().to_string(),
                audio_encoding: 1,
                environment: 1,
                client: Some(protocol::Client {
                    name: "Handy".into(),
                    platform: 6,
                    version: Some(protocol::Version {
                        major: 1,
                        minor: 6,
                        patch: 1174,
                    }),
                }),
            }),
            preferences: Some(protocol::Preferences {
                language: language_codes(language)?,
                vocabulary: Some(protocol::Vocabulary {
                    dictionary_personal: words.to_vec(),
                }),
            }),
        }),
        context: Some(protocol::Context {
            app: Some(protocol::App {
                name: "Handy".into(),
                kind: 1,
            }),
        }),
        commit: 2,
        ..Default::default()
    })
}

pub(crate) fn audio_request(samples: &[f32]) -> protocol::Request {
    // Build PCM16 rather than float bytes; each gRPC payload contains raw PCM.
    let bytes = samples
        .iter()
        .flat_map(|sample| {
            ((sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16).to_le_bytes()
        })
        .collect();
    protocol::Request {
        payload: Some(protocol::Payload {
            audio_packets: Some(protocol::AudioPackets { items: vec![bytes] }),
        }),
        commit: 2,
        ..Default::default()
    }
}

fn rpc_error(error: tonic::Status) -> anyhow::Error {
    match error.code() {
        tonic::Code::Unauthenticated | tonic::Code::PermissionDenied => {
            anyhow!("Wispr Flow rejected the session. Sign in again in transcription settings.")
        }
        _ => anyhow!("Wispr Flow streaming request failed ({:?}).", error.code()),
    }
}

pub(crate) struct Session {
    sender: Option<mpsc::UnboundedSender<protocol::Request>>,
    responses: tonic::Streaming<protocol::Response>,
    result: Option<protocol::Result>,
}

impl Session {
    pub(crate) async fn connect(
        credential: &Credential,
        language: &str,
        words: &[String],
    ) -> Result<Self> {
        let init = init_request(credential, language, words)?;
        let channel = Endpoint::from_static(client_config::TRANSCRIPTION_URL)
            .connect_timeout(Duration::from_secs(5))
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .connect()
            .await
            .map_err(|_| anyhow!("Could not connect to Wispr Flow."))?;
        Self::on_channel(channel, credential, init).await
    }

    async fn on_channel(
        channel: Channel,
        credential: &Credential,
        init: protocol::Request,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::unbounded_channel();
        sender
            .send(init)
            .map_err(|_| anyhow!("Wispr Flow stream closed before initialization."))?;
        let mut request = tonic::Request::new(UnboundedReceiverStream::new(receiver));
        for (key, value) in [
            (
                "authorization",
                format!("Bearer {}", credential.access_token),
            ),
            (
                "baseten-authorization",
                format!("Api-Key {}", client_config::TRANSPORT_KEY),
            ),
            (
                "baseten-model-id",
                format!("model-{}", client_config::MODEL_ID),
            ),
            ("x-baseten-environment", "production".into()),
            ("flow-debug", "false".into()),
            ("disable-formatting", "false".into()),
        ] {
            request.metadata_mut().insert(
                key,
                value
                    .parse()
                    .map_err(|_| anyhow!("Invalid Wispr Flow connection credential."))?,
            );
        }
        let mut grpc = tonic::client::Grpc::new(channel);
        grpc.ready()
            .await
            .map_err(|_| anyhow!("Wispr Flow connection is unavailable."))?;
        let response = tokio::time::timeout(
            Duration::from_secs(8),
            grpc.streaming(
                request,
                tonic::codegen::http::uri::PathAndQuery::from_static(METHOD),
                tonic::codec::ProstCodec::default(),
            ),
        )
        .await
        .map_err(|_| anyhow!("Wispr Flow handshake timed out."))?
        .map_err(rpc_error)?;
        Ok(Self {
            sender: Some(sender),
            responses: response.into_inner(),
            result: None,
        })
    }

    pub(crate) fn feed(&self, samples: &[f32]) -> Result<()> {
        if samples.is_empty() {
            return Ok(());
        }
        self.sender
            .as_ref()
            .context("Wispr Flow recording has already ended.")?
            .send(audio_request(samples))
            .map_err(|_| anyhow!("Wispr Flow stream closed during recording."))
    }

    pub(crate) async fn next(&mut self) -> Result<Option<protocol::Response>> {
        let response = self.responses.message().await.map_err(rpc_error)?;
        if let Some(result) = response.as_ref().and_then(|r| r.result.as_ref()) {
            self.result = Some(result.clone());
        }
        Ok(response)
    }

    pub(crate) async fn finish(&mut self, mut update: impl FnMut(&str, &str)) -> Result<String> {
        let sender = self
            .sender
            .take()
            .context("Wispr Flow recording has already ended.")?;
        sender
            .send(protocol::Request {
                commit: 1,
                ..Default::default()
            })
            .map_err(|_| anyhow!("Wispr Flow stream closed before commit."))?;
        drop(sender); // Half-close only after all queued PCM packets and commit.
        tokio::time::timeout(Duration::from_secs(20), async {
            while let Some(response) = self.next().await? {
                emit_response(&response, &mut update);
            }
            // Reading through EOF checks gRPC trailers before accepting text.
            let result = self
                .result
                .take()
                .context("Wispr Flow closed without a final transcription.")?;
            if !matches!(result.status, 1 | 3) {
                bail!("Wispr Flow could not transcribe this recording.");
            }
            Ok(result
                .output
                .context("Wispr Flow returned no transcription output.")?
                .plaintext
                .trim()
                .to_string())
        })
        .await
        .map_err(|_| anyhow!("Wispr Flow timed out finalizing the recording."))?
    }
}

pub(crate) fn emit_response(response: &protocol::Response, update: &mut impl FnMut(&str, &str)) {
    if let Some(state) = response
        .state
        .as_ref()
        .or_else(|| response.all_states.last())
    {
        let raw = state
            .raw_text
            .as_ref()
            .map(|t| t.content.as_str())
            .unwrap_or_default();
        let formatted = state
            .formatted_text
            .as_ref()
            .map(|t| t.content.as_str())
            .unwrap_or_default();
        if !formatted.is_empty() {
            update(formatted, "");
        } else if !raw.is_empty() {
            update("", raw);
        }
    }
}

async fn verify_credential(credential: &Credential) -> Result<()> {
    let mut session = Session::connect(credential, "auto", &[]).await?;
    session.feed(&vec![0.0; 16_000])?;
    session.finish(|_, _| {}).await?;
    Ok(())
}

#[cfg(test)]
mod tests;
