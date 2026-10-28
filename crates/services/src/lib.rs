pub mod deepseek;
pub mod process;
pub mod rich_text;
pub mod secrets;
pub mod sensors;
pub fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
