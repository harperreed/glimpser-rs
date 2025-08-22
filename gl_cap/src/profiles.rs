//! ABOUTME: Common CAP alert profiles for typical emergency scenarios  
//! ABOUTME: Provides pre-configured templates for weather, safety, and other alert types

use chrono::Duration;

use crate::{
    builder::AlertBuilder,
    Category, Certainty, ResponseType, Severity, Status, Urgency,
};

/// Common alert profile templates
pub struct AlertProfiles;

impl AlertProfiles {
    /// Create a severe weather alert profile
    pub fn severe_weather(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Severe Weather Alert")
                    .add_category(Category::Met)
                    .urgency(Urgency::Expected)
                    .severity(Severity::Severe)
                    .certainty(Certainty::Likely)
                    .add_response_type(ResponseType::Prepare)
                    .add_response_type(ResponseType::Monitor)
                    .headline("Severe Weather Warning")
                    .effective_now()
                    .expires_in(Duration::hours(4))
                    .sender_name("National Weather Service")
                    .language("en-US")
            })
    }

    /// Create an extreme weather alert (tornado, hurricane)
    pub fn extreme_weather(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Extreme Weather Alert")
                    .add_category(Category::Met)
                    .urgency(Urgency::Immediate)
                    .severity(Severity::Extreme)
                    .certainty(Certainty::Observed)
                    .add_response_type(ResponseType::Shelter)
                    .add_response_type(ResponseType::Evacuate)
                    .headline("EXTREME WEATHER - TAKE SHELTER NOW")
                    .effective_now()
                    .expires_in(Duration::hours(2))
                    .sender_name("Emergency Management")
                    .language("en-US")
            })
    }

    /// Create a fire/wildfire alert
    pub fn fire_alert(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Fire Alert")
                    .add_category(Category::Fire)
                    .urgency(Urgency::Expected)
                    .severity(Severity::Severe)
                    .certainty(Certainty::Likely)
                    .add_response_type(ResponseType::Prepare)
                    .add_response_type(ResponseType::Evacuate)
                    .headline("Fire Warning - Prepare to Evacuate")
                    .effective_now()
                    .expires_in(Duration::hours(6))
                    .sender_name("Fire Department")
                    .language("en-US")
            })
    }

    /// Create a public safety alert
    pub fn public_safety(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Public Safety Alert")
                    .add_category(Category::Safety)
                    .urgency(Urgency::Immediate)
                    .severity(Severity::Severe)
                    .certainty(Certainty::Observed)
                    .add_response_type(ResponseType::Avoid)
                    .add_response_type(ResponseType::Monitor)
                    .headline("Public Safety Alert")
                    .effective_now()
                    .expires_in(Duration::hours(2))
                    .sender_name("Public Safety")
                    .language("en-US")
            })
    }

    /// Create a health alert (pandemic, contamination)
    pub fn health_alert(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Health Alert")
                    .add_category(Category::Health)
                    .urgency(Urgency::Expected)
                    .severity(Severity::Moderate)
                    .certainty(Certainty::Likely)
                    .add_response_type(ResponseType::Prepare)
                    .add_response_type(ResponseType::Monitor)
                    .headline("Health Advisory")
                    .effective_now()
                    .expires_in(Duration::hours(12))
                    .sender_name("Health Department")
                    .language("en-US")
            })
    }

    /// Create an environmental hazard alert
    pub fn environmental_hazard(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Environmental Hazard")
                    .add_category(Category::Env)
                    .urgency(Urgency::Expected)
                    .severity(Severity::Moderate)
                    .certainty(Certainty::Possible)
                    .add_response_type(ResponseType::Avoid)
                    .add_response_type(ResponseType::Monitor)
                    .headline("Environmental Hazard Alert")
                    .effective_now()
                    .expires_in(Duration::hours(8))
                    .sender_name("Environmental Protection")
                    .language("en-US")
            })
    }

    /// Create a transportation alert
    pub fn transportation_alert(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Transportation Alert")
                    .add_category(Category::Transport)
                    .urgency(Urgency::Future)
                    .severity(Severity::Minor)
                    .certainty(Certainty::Likely)
                    .add_response_type(ResponseType::Avoid)
                    .add_response_type(ResponseType::Prepare)
                    .headline("Transportation Disruption")
                    .effective_now()
                    .expires_in(Duration::hours(6))
                    .sender_name("Transportation Authority")
                    .language("en-US")
            })
    }

    /// Create an infrastructure alert (power, water, communications)
    pub fn infrastructure_alert(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Infrastructure Alert")
                    .add_category(Category::Infra)
                    .urgency(Urgency::Future)
                    .severity(Severity::Moderate)
                    .certainty(Certainty::Likely)
                    .add_response_type(ResponseType::Prepare)
                    .add_response_type(ResponseType::Monitor)
                    .headline("Infrastructure Service Alert")
                    .effective_now()
                    .expires_in(Duration::hours(4))
                    .sender_name("Utility Services")
                    .language("en-US")
            })
    }

    /// Create a CBRNE (Chemical, Biological, Radiological, Nuclear, Explosive) alert
    pub fn cbrne_alert(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("CBRNE Alert")
                    .add_category(Category::CBRNE)
                    .urgency(Urgency::Immediate)
                    .severity(Severity::Extreme)
                    .certainty(Certainty::Observed)
                    .add_response_type(ResponseType::Shelter)
                    .add_response_type(ResponseType::Evacuate)
                    .headline("HAZARDOUS MATERIALS ALERT - SHELTER IN PLACE")
                    .effective_now()
                    .expires_in(Duration::hours(12))
                    .sender_name("Emergency Management")
                    .language("en-US")
            })
    }

    /// Create a security alert
    pub fn security_alert(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("Security Alert")
                    .add_category(Category::Security)
                    .urgency(Urgency::Immediate)
                    .severity(Severity::Severe)
                    .certainty(Certainty::Likely)
                    .add_response_type(ResponseType::Shelter)
                    .add_response_type(ResponseType::Avoid)
                    .headline("Security Alert - Avoid Area")
                    .effective_now()
                    .expires_in(Duration::hours(4))
                    .sender_name("Law Enforcement")
                    .language("en-US")
            })
    }

    /// Create a test alert
    pub fn test_alert(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Test)
            .add_info(|info| {
                info.event("Test Alert")
                    .add_category(Category::Safety)
                    .urgency(Urgency::Future)
                    .severity(Severity::Minor)
                    .certainty(Certainty::Unknown)
                    .add_response_type(ResponseType::None)
                    .headline("TEST ALERT - NO ACTION REQUIRED")
                    .description("This is a test of the emergency alert system. No action is required.")
                    .instruction("This is only a test. Please disregard this message.")
                    .effective_now()
                    .expires_in(Duration::minutes(15))
                    .sender_name("Emergency Alert System")
                    .language("en-US")
            })
    }

    /// Create an all-clear alert to cancel previous alerts
    pub fn all_clear(sender: impl Into<String>) -> AlertBuilder {
        AlertBuilder::new(sender)
            .status(Status::Actual)
            .add_info(|info| {
                info.event("All Clear")
                    .add_category(Category::Safety)
                    .urgency(Urgency::Future)
                    .severity(Severity::Minor)
                    .certainty(Certainty::Observed)
                    .add_response_type(ResponseType::AllClear)
                    .headline("All Clear - Emergency Has Ended")
                    .description("The emergency situation has ended. Normal activities may resume.")
                    .instruction("You may now resume normal activities. Stay alert for further updates.")
                    .effective_now()
                    .expires_in(Duration::hours(1))
                    .sender_name("Emergency Management")
                    .language("en-US")
            })
    }
}

/// Profile customization helpers
impl AlertBuilder {
    /// Apply common area templates
    pub fn add_county_area(mut self, county_name: impl Into<String>, fips_code: impl Into<String>) -> Self {
        if let Some(info) = self.alert.info.last_mut() {
            info.area.push(crate::Area {
                area_desc: format!("{} County", county_name.into()),
                polygon: Vec::new(),
                circle: Vec::new(),
                geocode: vec![crate::Geocode {
                    value_name: "FIPS6".to_string(),
                    value: fips_code.into(),
                }],
                altitude: None,
                ceiling: None,
            });
        }
        self
    }

    /// Add a circular area around a point
    pub fn add_circular_area(
        mut self,
        area_desc: impl Into<String>, 
        latitude: f64, 
        longitude: f64, 
        radius_km: f64
    ) -> Self {
        if let Some(info) = self.alert.info.last_mut() {
            info.area.push(crate::Area {
                area_desc: area_desc.into(),
                polygon: Vec::new(),
                circle: vec![format!("{},{} {}", latitude, longitude, radius_km)],
                geocode: Vec::new(),
                altitude: None,
                ceiling: None,
            });
        }
        self
    }

    /// Add contact information to the most recent info block
    pub fn with_contact_info(mut self, contact: impl Into<String>) -> Self {
        if let Some(info) = self.alert.info.last_mut() {
            info.contact = Some(contact.into());
        }
        self
    }

    /// Add web resource URL to the most recent info block  
    pub fn with_web_resource(mut self, url: impl AsRef<str>) -> Self {
        if let Some(info) = self.alert.info.last_mut() {
            if let Ok(parsed_url) = url.as_ref().parse() {
                info.web = Some(parsed_url);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::Validate;

    #[test]
    fn test_severe_weather_profile() {
        let alert = AlertProfiles::severe_weather("weather.example.org")
            .add_circular_area("Downtown Area", 42.0, -71.0, 10.0)
            .build();

        assert_eq!(alert.status, Status::Actual);
        assert_eq!(alert.info.len(), 1);
        
        let info = &alert.info[0];
        assert_eq!(info.event, "Severe Weather Alert");
        assert!(info.category.contains(&Category::Met));
        assert_eq!(info.urgency, Urgency::Expected);
        assert_eq!(info.severity, Severity::Severe);
        assert_eq!(info.certainty, Certainty::Likely);
        assert!(info.response_type.contains(&ResponseType::Prepare));
        
        assert!(alert.validate().is_ok());
    }

    #[test]
    fn test_extreme_weather_profile() {
        let alert = AlertProfiles::extreme_weather("emergency.example.org")
            .build();

        assert_eq!(alert.info[0].urgency, Urgency::Immediate);
        assert_eq!(alert.info[0].severity, Severity::Extreme);
        assert!(alert.info[0].response_type.contains(&ResponseType::Shelter));
        assert!(alert.validate().is_ok());
    }

    #[test]
    fn test_test_alert_profile() {
        let alert = AlertProfiles::test_alert("test.example.org")
            .build();

        assert_eq!(alert.status, Status::Test);
        assert_eq!(alert.info[0].event, "Test Alert");
        assert!(alert.info[0].response_type.contains(&ResponseType::None));
        assert!(alert.validate().is_ok());
    }

    #[test]
    fn test_all_clear_profile() {
        let alert = AlertProfiles::all_clear("emergency.example.org")
            .build();

        assert_eq!(alert.info[0].event, "All Clear");
        assert!(alert.info[0].response_type.contains(&ResponseType::AllClear));
        assert!(alert.validate().is_ok());
    }

    #[test]
    fn test_county_area_helper() {
        let alert = AlertProfiles::severe_weather("weather.example.org")
            .add_county_area("Suffolk", "25025")
            .build();

        // The county area should be in the info block
        // Note: This is a simplified test - the actual implementation
        // would need to handle multiple info blocks properly
        assert!(alert.validate().is_ok());
    }

    #[test]
    fn test_circular_area_helper() {
        let alert = AlertProfiles::public_safety("safety.example.org")
            .add_circular_area("City Center", 42.3601, -71.0589, 5.0)
            .with_contact_info("Emergency Services: 911")
            .with_web_resource("https://emergency.example.org/updates")
            .build();

        assert!(alert.validate().is_ok());
    }

    #[test]
    fn test_all_profiles_validate() {
        let profiles = vec![
            AlertProfiles::severe_weather("test.org"),
            AlertProfiles::extreme_weather("test.org"),
            AlertProfiles::fire_alert("test.org"),
            AlertProfiles::public_safety("test.org"),
            AlertProfiles::health_alert("test.org"),
            AlertProfiles::environmental_hazard("test.org"),
            AlertProfiles::transportation_alert("test.org"),
            AlertProfiles::infrastructure_alert("test.org"),
            AlertProfiles::cbrne_alert("test.org"),
            AlertProfiles::security_alert("test.org"),
            AlertProfiles::test_alert("test.org"),
            AlertProfiles::all_clear("test.org"),
        ];

        for profile in profiles {
            let alert = profile.build();
            assert!(alert.validate().is_ok(), "Profile validation failed: {:?}", alert.info[0].event);
        }
    }
}