use super::*;
use tonic::codegen::*;

fn synthetic_credential() -> Credential {
    let claims =
        serde_json::json!({"sub": "00000000-0000-4000-8000-000000000001", "exp": 4000000000_u64});
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
    Credential {
        access_token: format!("test.{payload}.test"),
        refresh_token: "synthetic-refresh".into(),
        provider: "supabase".into(),
    }
}

#[test]
fn init_maps_languages_and_custom_words() {
    let request = init_request(&synthetic_credential(), "zh-Hans", &["TurnKey".into()]).unwrap();
    let init = request.init.unwrap();
    let preferences = init.preferences.unwrap();
    assert_eq!(preferences.language, vec![3]);
    assert_eq!(
        preferences.vocabulary.unwrap().dictionary_personal,
        vec!["TurnKey"]
    );
    assert_eq!(init.metadata.unwrap().audio_encoding, 1);
    assert_eq!(request.commit, 2);
    assert!(language_codes("auto").unwrap().is_empty());
    assert!(language_codes("unrecognized-language").is_err());
}

#[test]
fn audio_is_pcm16_without_wav_header_or_float_bytes() {
    let request = audio_request(&[-1.0, 0.0, 1.0, 2.0]);
    let packets = request.payload.unwrap().audio_packets.unwrap().items;
    assert_eq!(packets, vec![vec![1, 128, 0, 0, 255, 127, 255, 127]]);
    assert_eq!(request.commit, 2);
}

#[test]
fn session_errors_never_echo_remote_details() {
    let error = rpc_error(tonic::Status::permission_denied(
        "private-token-and-user-info",
    ));
    assert!(!error.to_string().contains("private-token"));
    assert!(error.to_string().contains("Sign in again"));
}

type Observed = std::sync::Arc<std::sync::Mutex<Vec<protocol::Request>>>;

#[derive(Clone)]
struct MockService {
    fail_trailers: bool,
    observed: Observed,
}
impl tonic::server::NamedService for MockService {
    const NAME: &'static str = "flow_api.v1.TranscriptionService";
}
impl<B> Service<http::Request<B>> for MockService
where
    B: Body + Send + 'static,
    B::Error: Into<StdError> + Send + 'static,
{
    type Response = http::Response<tonic::body::BoxBody>;
    type Error = std::convert::Infallible;
    type Future = BoxFuture<Self::Response, Self::Error>;
    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn call(&mut self, request: http::Request<B>) -> Self::Future {
        assert_eq!(request.uri().path(), METHOD);
        let service = self.clone();
        Box::pin(async move {
            let codec = tonic::codec::ProstCodec::default();
            Ok(tonic::server::Grpc::new(codec)
                .streaming(service, request)
                .await)
        })
    }
}
impl tonic::server::StreamingService<protocol::Request> for MockService {
    type Response = protocol::Response;
    type ResponseStream = tokio_stream::wrappers::ReceiverStream<
        std::result::Result<protocol::Response, tonic::Status>,
    >;
    type Future = BoxFuture<tonic::Response<Self::ResponseStream>, tonic::Status>;
    fn call(
        &mut self,
        request: tonic::Request<tonic::Streaming<protocol::Request>>,
    ) -> Self::Future {
        let observed = self.observed.clone();
        let fail = self.fail_trailers;
        Box::pin(async move {
            assert!(request.metadata().get("authorization").is_some());
            let mut requests = request.into_inner();
            let (tx, rx) = mpsc::channel(8);
            tokio::spawn(async move {
                while let Some(request) = requests.message().await.unwrap() {
                    let commit = request.commit == 1;
                    observed.lock().unwrap().push(request);
                    if commit {
                        assert!(
                            requests.message().await.unwrap().is_none(),
                            "commit must half-close requests"
                        );
                        tx.send(Ok(protocol::Response {
                            result: Some(protocol::Result {
                                status: 1,
                                output: Some(protocol::Transcription {
                                    plaintext: "Final text.".into(),
                                }),
                            }),
                            ..Default::default()
                        }))
                        .await
                        .unwrap();
                        if fail {
                            let _ = tx
                                .send(Err(tonic::Status::permission_denied(
                                    "private remote details",
                                )))
                                .await;
                        }
                        break;
                    }
                }
            });
            Ok(tonic::Response::new(
                tokio_stream::wrappers::ReceiverStream::new(rx),
            ))
        })
    }
}

async fn mock_session(fail_trailers: bool) -> (Session, Observed, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let service = MockService {
        fail_trailers,
        observed: observed.clone(),
    };
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(service)
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    let channel = Endpoint::from_shared(format!("http://{address}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let credential = synthetic_credential();
    let session = Session::on_channel(
        channel,
        &credential,
        init_request(&credential, "en", &[]).unwrap(),
    )
    .await
    .unwrap();
    (session, observed, server)
}

#[tokio::test]
async fn stream_sends_all_pcm_before_commit_and_checks_final_result() {
    let (mut session, observed, server) = mock_session(false).await;
    session.feed(&[0.0; 1600]).unwrap();
    session.feed(&[0.25; 1600]).unwrap();
    assert_eq!(session.finish(|_, _| {}).await.unwrap(), "Final text.");
    let requests = observed.lock().unwrap();
    assert!(requests[0].init.is_some());
    assert_eq!(requests.len(), 4);
    assert!(requests[1].payload.is_some());
    assert!(requests[2].payload.is_some());
    assert_eq!(requests[3].commit, 1);
    server.abort();
}

#[tokio::test]
async fn text_is_rejected_if_final_grpc_status_fails() {
    let (mut session, _, server) = mock_session(true).await;
    session.feed(&[0.0; 1600]).unwrap();
    let error = session.finish(|_, _| {}).await.unwrap_err().to_string();
    assert!(error.contains("Sign in again"));
    assert!(!error.contains("private remote details"));
    server.abort();
}

#[tokio::test]
#[ignore = "requires the account owner's private credentials and live Wispr access"]
async fn live_streaming_fixture() {
    let path =
        std::env::var("HANDY_WISPR_TEST_CREDENTIALS").expect("credential file path required");
    let credential: Credential = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let wav = std::env::var("HANDY_WISPR_TEST_WAV").expect("WAV path required");
    let mut reader = hound::WavReader::open(wav).unwrap();
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();
    let mut session = Session::connect(&credential, "en", &[]).await.unwrap();
    for chunk in samples.chunks(1600) {
        session.feed(chunk).unwrap();
        tokio::time::sleep(Duration::from_secs_f64(chunk.len() as f64 / 16000.0)).await;
    }
    let started = std::time::Instant::now();
    let text = session.finish(|_, _| {}).await.unwrap();
    assert!(text.to_lowercase().contains("yellow lamps"));
    println!(
        "Wispr native gRPC finalization: {} ms; text: {}",
        started.elapsed().as_millis(),
        text
    );
    verify_credential(&credential).await.unwrap();
}

#[test]
fn browser_device_login_accepts_complete_session_and_waits_on_pending() {
    let pending: DeviceLogin =
        serde_json::from_value(serde_json::json!({"status": "pending"})).unwrap();
    assert_eq!(pending.status, "pending");
    assert!(pending.credential.is_none());
    let credential = synthetic_credential();
    let complete: DeviceLogin = serde_json::from_value(serde_json::json!({
        "status": "complete", "access_token": credential.access_token,
        "refresh_token": credential.refresh_token, "provider": "supabase",
        "email": "profile-not-retained@example.test"
    }))
    .unwrap();
    let saved = complete.credential.unwrap();
    assert!(saved.claims().is_ok());
    assert!(!serde_json::to_string(&saved)
        .unwrap()
        .contains("profile-not-retained"));
}
