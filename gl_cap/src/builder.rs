//! ABOUTME: CAP message builder for fluent construction of alerts
//! ABOUTME: Provides a convenient API for creating valid CAP messages

use chrono::{DateTime, Duration, Utc};
use gl_core::Id;

use crate::{
    Alert, Area, Category, Certainty, Geocode, Info, MsgType, Resource, ResponseType, Scope,
    Severity, Status, Urgency,
};

/// Builder for CAP Alert messages
pub struct AlertBuilder {
    pub(crate) alert: Alert,
}

impl AlertBuilder {
    /// Create a new AlertBuilder
    pub fn new(sender: impl Into<String>) -> Self {
        let identifier = Id::new().to_string();
        let sender = sender.into();

        Self {
            alert: Alert::new(identifier, sender),
        }
    }

    /// Set a custom identifier (defaults to generated ULID)
    pub fn identifier(mut self, identifier: impl Into<String>) -> Self {
        self.alert.identifier = identifier.into();
        self
    }

    /// Set the alert status
    pub fn status(mut self, status: Status) -> Self {
        self.alert.status = status;
        self
    }

    /// Set the message type
    pub fn msg_type(mut self, msg_type: MsgType) -> Self {
        self.alert.msg_type = msg_type;
        self
    }

    /// Set the scope
    pub fn scope(mut self, scope: Scope) -> Self {
        self.alert.scope = scope;
        self
    }

    /// Set the source
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.alert.source = Some(source.into());
        self
    }

    /// Set restriction (required when scope is Restricted)
    pub fn restriction(mut self, restriction: impl Into<String>) -> Self {
        self.alert.restriction = Some(restriction.into());
        self
    }

    /// Set addresses (required when scope is Private)
    pub fn addresses(mut self, addresses: impl Into<String>) -> Self {
        self.alert.addresses = Some(addresses.into());
        self
    }

    /// Add a handling code
    pub fn add_code(mut self, code: impl Into<String>) -> Self {
        let code = code.into();
        if let Some(ref mut codes) = self.alert.code {
            codes.push(code);
        } else {
            self.alert.code = Some(vec![code]);
        }
        self
    }

    /// Set a note
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.alert.note = Some(note.into());
        self
    }

    /// Set references to other alerts
    pub fn references(mut self, references: impl Into<String>) -> Self {
        self.alert.references = Some(references.into());
        self
    }

    /// Set related incidents
    pub fn incidents(mut self, incidents: impl Into<String>) -> Self {
        self.alert.incidents = Some(incidents.into());
        self
    }

    /// Add an Info block using an InfoBuilder
    pub fn add_info<F>(mut self, f: F) -> Self
    where
        F: FnOnce(InfoBuilder) -> InfoBuilder,
    {
        let info_builder = InfoBuilder::new();
        let info = f(info_builder).build();
        self.alert.info.push(info);
        self
    }

    /// Build the Alert
    pub fn build(self) -> Alert {
        self.alert
    }
}

/// Builder for CAP Info blocks
pub struct InfoBuilder {
    info: Info,
}

impl InfoBuilder {
    /// Create a new InfoBuilder
    pub fn new() -> Self {
        Self {
            info: Info::new(
                "Unknown Event".to_string(),
                Urgency::Unknown,
                Severity::Unknown,
                Certainty::Unknown,
            ),
        }
    }

    /// Set the event type
    pub fn event(mut self, event: impl Into<String>) -> Self {
        self.info.event = event.into();
        self
    }

    /// Set the language
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.info.language = Some(language.into());
        self
    }

    /// Add a category
    pub fn add_category(mut self, category: Category) -> Self {
        let wrapper = crate::CategoryWrapper::from(category);
        if !self.info.category.contains(&wrapper) {
            self.info.category.push(wrapper);
        }
        self
    }

    /// Set categories (replaces existing)
    pub fn categories(mut self, categories: Vec<Category>) -> Self {
        self.info.category = categories
            .into_iter()
            .map(crate::CategoryWrapper::from)
            .collect();
        self
    }

    /// Add a response type
    pub fn add_response_type(mut self, response_type: ResponseType) -> Self {
        let wrapper = crate::ResponseTypeWrapper::from(response_type);
        if !self.info.response_type.contains(&wrapper) {
            self.info.response_type.push(wrapper);
        }
        self
    }

    /// Set urgency
    pub fn urgency(mut self, urgency: Urgency) -> Self {
        self.info.urgency = urgency;
        self
    }

    /// Set severity
    pub fn severity(mut self, severity: Severity) -> Self {
        self.info.severity = severity;
        self
    }

    /// Set certainty
    pub fn certainty(mut self, certainty: Certainty) -> Self {
        self.info.certainty = certainty;
        self
    }

    /// Set the headline
    pub fn headline(mut self, headline: impl Into<String>) -> Self {
        self.info.headline = Some(headline.into());
        self
    }

    /// Set the description
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.info.description = Some(description.into());
        self
    }

    /// Set the instruction
    pub fn instruction(mut self, instruction: impl Into<String>) -> Self {
        self.info.instruction = Some(instruction.into());
        self
    }

    /// Set the sender name
    pub fn sender_name(mut self, sender_name: impl Into<String>) -> Self {
        self.info.sender_name = Some(sender_name.into());
        self
    }

    /// Set the audience
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        self.info.audience = Some(audience.into());
        self
    }

    /// Set the contact information
    pub fn contact(mut self, contact: impl Into<String>) -> Self {
        self.info.contact = Some(contact.into());
        self
    }

    /// Set effective time
    pub fn effective(mut self, effective: DateTime<Utc>) -> Self {
        self.info.effective = Some(effective);
        self
    }

    /// Set effective time to now
    pub fn effective_now(mut self) -> Self {
        self.info.effective = Some(Utc::now());
        self
    }

    /// Set onset time
    pub fn onset(mut self, onset: DateTime<Utc>) -> Self {
        self.info.onset = Some(onset);
        self
    }

    /// Set onset time relative to now
    pub fn onset_in(mut self, duration: Duration) -> Self {
        self.info.onset = Some(Utc::now() + duration);
        self
    }

    /// Set expires time
    pub fn expires(mut self, expires: DateTime<Utc>) -> Self {
        self.info.expires = Some(expires);
        self
    }

    /// Set expires time relative to now
    pub fn expires_in(mut self, duration: Duration) -> Self {
        self.info.expires = Some(Utc::now() + duration);
        self
    }

    /// Add an area using an AreaBuilder
    pub fn add_area<F>(mut self, f: F) -> Self
    where
        F: FnOnce(AreaBuilder) -> AreaBuilder,
    {
        let area_builder = AreaBuilder::new("Area".to_string());
        let area = f(area_builder).build();
        self.info.area.push(area);
        self
    }

    /// Add a resource
    pub fn add_resource(mut self, resource: Resource) -> Self {
        self.info.resource.push(resource);
        self
    }

    /// Build the Info block
    pub fn build(self) -> Info {
        self.info
    }
}

impl Default for InfoBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for CAP Area blocks
pub struct AreaBuilder {
    area: Area,
}

impl AreaBuilder {
    /// Create a new AreaBuilder
    pub fn new(area_desc: String) -> Self {
        Self {
            area: Area {
                area_desc,
                polygon: Vec::new(),
                circle: Vec::new(),
                geocode: Vec::new(),
                altitude: None,
                ceiling: None,
            },
        }
    }

    /// Set the area description
    pub fn area_desc(mut self, area_desc: impl Into<String>) -> Self {
        self.area.area_desc = area_desc.into();
        self
    }

    /// Add a polygon (latitude,longitude pairs)
    pub fn add_polygon(mut self, polygon: impl Into<String>) -> Self {
        self.area.polygon.push(polygon.into());
        self
    }

    /// Add a circle (latitude,longitude radius)
    pub fn add_circle(mut self, circle: impl Into<String>) -> Self {
        self.area.circle.push(circle.into());
        self
    }

    /// Add a geocode
    pub fn add_geocode(mut self, value_name: impl Into<String>, value: impl Into<String>) -> Self {
        self.area.geocode.push(Geocode {
            value_name: value_name.into(),
            value: value.into(),
        });
        self
    }

    /// Set altitude
    pub fn altitude(mut self, altitude: f64) -> Self {
        self.area.altitude = Some(altitude);
        self
    }

    /// Set ceiling
    pub fn ceiling(mut self, ceiling: f64) -> Self {
        self.area.ceiling = Some(ceiling);
        self
    }

    /// Build the Area
    pub fn build(self) -> Area {
        self.area
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_alert_builder() {
        let alert = AlertBuilder::new("example.org")
            .status(Status::Test)
            .msg_type(MsgType::Alert)
            .scope(Scope::Public)
            .note("This is a test alert")
            .add_code("TEST")
            .add_info(|info| {
                info.event("Test Event")
                    .urgency(Urgency::Future)
                    .severity(Severity::Minor)
                    .certainty(Certainty::Possible)
                    .headline("Test Alert Headline")
                    .description("This is a test alert for the CAP system")
                    .effective_now()
                    .expires_in(Duration::hours(1))
                    .add_area(|area| {
                        area.area_desc("Test Area")
                            .add_polygon("42.0,-71.0 42.1,-71.0 42.1,-70.9 42.0,-70.9 42.0,-71.0")
                            .add_geocode("FIPS6", "123456")
                    })
            })
            .build();

        assert_eq!(alert.sender, "example.org");
        assert_eq!(alert.status, Status::Test);
        assert_eq!(alert.msg_type, MsgType::Alert);
        assert_eq!(alert.scope, Scope::Public);
        assert_eq!(alert.note, Some("This is a test alert".to_string()));
        assert!(alert.code.is_some());
        assert_eq!(alert.code.as_ref().unwrap()[0], "TEST");

        assert_eq!(alert.info.len(), 1);
        let info = &alert.info[0];
        assert_eq!(info.event, "Test Event");
        assert_eq!(info.urgency, Urgency::Future);
        assert_eq!(info.severity, Severity::Minor);
        assert_eq!(info.certainty, Certainty::Possible);
        assert_eq!(info.headline, Some("Test Alert Headline".to_string()));

        assert_eq!(info.area.len(), 1);
        let area = &info.area[0];
        assert_eq!(area.area_desc, "Test Area");
        assert_eq!(area.polygon.len(), 1);
        assert_eq!(area.geocode.len(), 1);
        assert_eq!(area.geocode[0].value_name, "FIPS6");
        assert_eq!(area.geocode[0].value, "123456");
    }

    #[test]
    fn test_xml_serialization() {
        let alert = AlertBuilder::new("test.example.org")
            .add_info(|info| {
                info.event("Test Event")
                    .add_category(Category::Safety)
                    .urgency(Urgency::Immediate)
                    .severity(Severity::Extreme)
                    .certainty(Certainty::Observed)
                    .headline("Emergency Test")
                    .description("This is an emergency test")
            })
            .build();

        let xml = alert.to_xml().expect("Should serialize to XML");
        println!("Generated XML:\n{}", xml);

        assert!(xml.contains("test.example.org"));
        assert!(xml.contains("Test Event"));
        assert!(xml.contains("Emergency Test"));

        // Test deserialization roundtrip works
        let parsed_alert = Alert::from_xml(&xml).expect("Should parse XML");
        assert_eq!(parsed_alert.sender, "test.example.org");
        assert_eq!(parsed_alert.info[0].event, "Test Event");
    }
}
