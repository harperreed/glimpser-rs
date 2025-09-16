//! ABOUTME: Notification adapter implementations for different channels
//! ABOUTME: Contains Webhook and Pushover notification adapters

pub mod pushover;
pub mod webhook;

pub use pushover::PushoverAdapter;
pub use webhook::WebhookAdapter;
