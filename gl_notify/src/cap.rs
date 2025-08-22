//! ABOUTME: CAP (Common Alerting Protocol) integration for notifications
//! ABOUTME: Converts CAP alerts to notifications with XML attachments or body content

use gl_cap::{Alert, profiles::AlertProfiles};
use crate::{Notification, NotificationChannel, NotificationKind, Result};
use url::Url;

/// Extension trait for creating notifications from CAP alerts
pub trait CapNotification {
    /// Create a notification from a CAP alert with XML as body content
    fn to_notification_with_body(
        self,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification>;

    /// Create a notification from a CAP alert with XML as attachment
    fn to_notification_with_attachment(
        self,
        attachment_url: Url,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification>;
}

impl CapNotification for Alert {
    fn to_notification_with_body(
        self,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        // Extract title and body from the first info block
        let (title, body) = if let Some(info) = self.info.first() {
            let title = info.headline.clone()
                .unwrap_or_else(|| format!("CAP Alert: {}", info.event));
            
            let body_text = info.description.clone()
                .or_else(|| info.instruction.clone())
                .unwrap_or_else(|| "Emergency alert - see CAP XML for details".to_string());
            
            (title, body_text)
        } else {
            ("CAP Alert".to_string(), "Emergency alert".to_string())
        };

        // Convert CAP urgency/severity to notification kind
        let kind = if let Some(info) = self.info.first() {
            match (&info.urgency, &info.severity) {
                (gl_cap::Urgency::Immediate, gl_cap::Severity::Extreme) => NotificationKind::Error,
                (gl_cap::Urgency::Immediate, _) => NotificationKind::Error,
                (_, gl_cap::Severity::Extreme) => NotificationKind::Error,
                (_, gl_cap::Severity::Severe) => NotificationKind::Warning,
                _ => NotificationKind::Info,
            }
        } else {
            NotificationKind::Warning
        };

        // Generate CAP XML
        let cap_xml = self.to_xml()
            .map_err(|e| crate::NotificationError::SerializationError(
                serde_json::Error::io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("CAP XML error: {}", e)
                ))
            ))?;

        let mut notification = Notification::new(kind, title, body, channels);

        // Add CAP XML to metadata
        notification.metadata.insert("cap_xml".to_string(), cap_xml);
        notification.metadata.insert("cap_identifier".to_string(), self.identifier.clone());
        notification.metadata.insert("cap_sender".to_string(), self.sender.clone());
        notification.metadata.insert("cap_status".to_string(), format!("{:?}", self.status));

        Ok(notification)
    }

    fn to_notification_with_attachment(
        self,
        attachment_url: Url,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        let mut notification = self.to_notification_with_body(channels)?;
        notification.attachments.push(attachment_url);
        Ok(notification)
    }
}

/// Helper functions for creating CAP-based notifications using profiles
pub struct CapNotificationBuilder;

impl CapNotificationBuilder {
    /// Create a severe weather notification
    pub fn severe_weather(
        sender: impl Into<String>,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        let alert = AlertProfiles::severe_weather(sender).build();
        alert.to_notification_with_body(channels)
    }

    /// Create an extreme weather notification
    pub fn extreme_weather(
        sender: impl Into<String>,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        let alert = AlertProfiles::extreme_weather(sender).build();
        alert.to_notification_with_body(channels)
    }

    /// Create a fire alert notification
    pub fn fire_alert(
        sender: impl Into<String>,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        let alert = AlertProfiles::fire_alert(sender).build();
        alert.to_notification_with_body(channels)
    }

    /// Create a health alert notification
    pub fn health_alert(
        sender: impl Into<String>,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        let alert = AlertProfiles::health_alert(sender).build();
        alert.to_notification_with_body(channels)
    }

    /// Create a public safety notification
    pub fn public_safety(
        sender: impl Into<String>,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        let alert = AlertProfiles::public_safety(sender).build();
        alert.to_notification_with_body(channels)
    }

    /// Create a test alert notification
    pub fn test_alert(
        sender: impl Into<String>,
        channels: Vec<NotificationChannel>,
    ) -> Result<Notification> {
        let alert = AlertProfiles::test_alert(sender).build();
        alert.to_notification_with_body(channels)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gl_cap::builder::AlertBuilder;
    use gl_cap::{Category, Certainty, Severity, Urgency};

    #[test]
    fn test_cap_alert_to_notification() {
        let alert = AlertBuilder::new("emergency.example.org")
            .add_info(|info| {
                info.event("Test Emergency")
                    .add_category(Category::Safety)
                    .urgency(Urgency::Immediate)
                    .severity(Severity::Extreme)
                    .certainty(Certainty::Observed)
                    .headline("Emergency Test Alert")
                    .description("This is a test emergency alert")
                    .instruction("Take immediate shelter")
            })
            .build();

        let channels = vec![NotificationChannel::Pushover {
            user_key: "test_user".to_string(),
            device: None,
            priority: Some(2),
            sound: Some("siren".to_string()),
        }];

        let notification = alert.to_notification_with_body(channels).unwrap();

        assert_eq!(notification.title, "Emergency Test Alert");
        assert_eq!(notification.body, "This is a test emergency alert");
        assert_eq!(notification.kind, NotificationKind::Error);
        assert!(notification.metadata.contains_key("cap_xml"));
        assert_eq!(notification.metadata.get("cap_sender"), Some(&"emergency.example.org".to_string()));
    }

    #[test]
    fn test_cap_notification_builder_severe_weather() {
        let channels = vec![NotificationChannel::Webhook {
            url: "https://example.com/webhook".parse().unwrap(),
            headers: None,
            method: None,
        }];

        let notification = CapNotificationBuilder::severe_weather("weather.example.org", channels).unwrap();

        assert_eq!(notification.title, "Severe Weather Warning");
        assert_eq!(notification.kind, NotificationKind::Warning);
        assert!(notification.metadata.contains_key("cap_xml"));
        assert!(notification.metadata.get("cap_xml").unwrap().contains("Severe Weather Alert"));
    }

    #[test]
    fn test_cap_alert_with_attachment() {
        let alert = AlertProfiles::test_alert("test.example.org").build();
        let attachment_url: Url = "https://example.com/cap/alert.xml".parse().unwrap();
        
        let channels = vec![NotificationChannel::Pushover {
            user_key: "test_user".to_string(),
            device: None,
            priority: None,
            sound: None,
        }];

        let notification = alert.to_notification_with_attachment(attachment_url.clone(), channels).unwrap();

        assert_eq!(notification.attachments.len(), 1);
        assert_eq!(notification.attachments[0], attachment_url);
        assert!(notification.metadata.contains_key("cap_xml"));
    }

    #[test]
    fn test_notification_kind_mapping() {
        // Test extreme + immediate = Error
        let alert = AlertBuilder::new("test.org")
            .add_info(|info| {
                info.event("Extreme Event")
                    .urgency(Urgency::Immediate)
                    .severity(Severity::Extreme)
                    .certainty(Certainty::Observed)
            })
            .build();

        let channels = vec![NotificationChannel::Webhook {
            url: "https://example.com/webhook".parse().unwrap(),
            headers: None,
            method: None,
        }];

        let notification = alert.to_notification_with_body(channels).unwrap();
        assert_eq!(notification.kind, NotificationKind::Error);

        // Test moderate severity = Info
        let alert = AlertBuilder::new("test.org")
            .add_info(|info| {
                info.event("Moderate Event")
                    .urgency(Urgency::Future)
                    .severity(Severity::Moderate)
                    .certainty(Certainty::Possible)
            })
            .build();

        let channels = vec![NotificationChannel::Webhook {
            url: "https://example.com/webhook".parse().unwrap(),
            headers: None,
            method: None,
        }];

        let notification = alert.to_notification_with_body(channels).unwrap();
        assert_eq!(notification.kind, NotificationKind::Info);
    }
}