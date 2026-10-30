// Supply service client settings through private build environment variables.
// A signed account session is additionally required by the service.
// Wispr's public inference gateway accepts current account sessions.
pub const TRANSCRIPTION_URL: &str = "https://inference.wisprflow.com";
pub const MODEL_ID: &str = "v31pl413";
pub const TRANSPORT_KEY: &str = match option_env!("HANDY_WISPR_TRANSPORT_KEY") {
    Some(value) => value,
    None => "",
};
pub const AUTH_URL: &str =
    "https://dodjkfqhwrzqjwkfnthl.supabase.co/auth/v1/token?grant_type=refresh_token";
pub const AUTH_ANON_KEY: &str = match option_env!("HANDY_WISPR_AUTH_ANON_KEY") {
    Some(value) => value,
    None => "",
};
