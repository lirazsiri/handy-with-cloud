//! Wire-compatible subset of the Wispr Flow 1.6.1174 desktop protocol.
//! Field numbers come from the client descriptors verified by the live probe.
#[derive(Clone, PartialEq, prost::Message)]
pub struct Version {
    #[prost(uint32, tag = "1")]
    pub major: u32,
    #[prost(uint32, tag = "2")]
    pub minor: u32,
    #[prost(uint32, tag = "3")]
    pub patch: u32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Client {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int32, tag = "2")]
    pub platform: i32,
    #[prost(message, optional, tag = "3")]
    pub version: Option<Version>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Metadata {
    #[prost(string, tag = "1")]
    pub user_id: String,
    #[prost(string, tag = "2")]
    pub session_id: String,
    #[prost(string, tag = "3")]
    pub request_id: String,
    #[prost(int32, tag = "4")]
    pub audio_encoding: i32,
    #[prost(int32, tag = "5")]
    pub environment: i32,
    #[prost(message, optional, tag = "6")]
    pub client: Option<Client>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct App {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int32, tag = "4")]
    pub kind: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Context {
    #[prost(message, optional, tag = "1")]
    pub app: Option<App>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Vocabulary {
    #[prost(string, repeated, tag = "1")]
    pub dictionary_personal: Vec<String>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Preferences {
    #[prost(int32, repeated, packed = "true", tag = "2")]
    pub language: Vec<i32>,
    #[prost(message, optional, tag = "3")]
    pub vocabulary: Option<Vocabulary>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Init {
    #[prost(message, optional, tag = "1")]
    pub metadata: Option<Metadata>,
    #[prost(message, optional, tag = "2")]
    pub preferences: Option<Preferences>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct AudioPackets {
    #[prost(bytes = "vec", repeated, tag = "1")]
    pub items: Vec<Vec<u8>>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Payload {
    #[prost(message, optional, tag = "1")]
    pub audio_packets: Option<AudioPackets>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Request {
    #[prost(message, optional, tag = "1")]
    pub init: Option<Init>,
    #[prost(message, optional, tag = "2")]
    pub context: Option<Context>,
    #[prost(message, optional, tag = "3")]
    pub payload: Option<Payload>,
    #[prost(int32, tag = "4")]
    pub commit: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Text {
    #[prost(string, tag = "1")]
    pub content: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct State {
    #[prost(message, optional, tag = "2")]
    pub raw_text: Option<Text>,
    #[prost(message, optional, tag = "3")]
    pub formatted_text: Option<Text>,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Transcription {
    #[prost(string, tag = "2")]
    pub plaintext: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Result {
    #[prost(message, optional, tag = "1")]
    pub output: Option<Transcription>,
    #[prost(int32, tag = "5")]
    pub status: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Response {
    #[prost(message, optional, tag = "1")]
    pub result: Option<Result>,
    #[prost(message, optional, tag = "2")]
    pub state: Option<State>,
    #[prost(message, repeated, tag = "3")]
    pub all_states: Vec<State>,
}
