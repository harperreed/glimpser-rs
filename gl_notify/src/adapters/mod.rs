//! ABOUTME: Notification adapter implementations for different channels
//! ABOUTME: Contains Webhook, WebPush, and Pushover notification adapters

pub mod pushover;
pub mod webhook;

#[cfg(feature = "webpush")]
pub mod webpush;

pub use pushover::PushoverAdapter;
pub use webhook::WebhookAdapter;

#[cfg(feature = "webpush")]
pub use webpush::WebPushAdapter;
