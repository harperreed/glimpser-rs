//! ABOUTME: Notification adapter implementations for different channels
//! ABOUTME: Contains Webhook, WebPush, and Pushover notification adapters

pub mod webhook;
pub mod pushover;

#[cfg(feature = "webpush")]
pub mod webpush;

pub use webhook::WebhookAdapter;
pub use pushover::PushoverAdapter;

#[cfg(feature = "webpush")]
pub use webpush::WebPushAdapter;