//! ABOUTME: CAP message validation helpers for required fields and formats
//! ABOUTME: Ensures CAP messages conform to specification requirements

use chrono::{DateTime, Utc};

use crate::{Alert, Area, CapError, Info, Result, Scope};

/// Validation trait for CAP components
pub trait Validate {
    /// Validate the component
    fn validate(&self) -> Result<()>;
}

impl Validate for Alert {
    fn validate(&self) -> Result<()> {
        // Required fields
        if self.identifier.is_empty() {
            return Err(CapError::MissingField("identifier".to_string()));
        }
        
        if self.sender.is_empty() {
            return Err(CapError::MissingField("sender".to_string()));
        }
        
        // Validate sender format (should be in domain format)
        if !is_valid_sender(&self.sender) {
            return Err(CapError::InvalidValue {
                field: "sender".to_string(),
                value: self.sender.clone(),
            });
        }
        
        // Validate scope-specific requirements
        match self.scope {
            Scope::Restricted if self.restriction.is_none() => {
                return Err(CapError::MissingField("restriction".to_string()));
            }
            Scope::Private if self.addresses.is_none() => {
                return Err(CapError::MissingField("addresses".to_string()));
            }
            _ => {}
        }
        
        // Validate sent timestamp is not in the future (with some tolerance)
        let now = Utc::now();
        if self.sent > now + chrono::Duration::minutes(5) {
            return Err(CapError::InvalidValue {
                field: "sent".to_string(),
                value: self.sent.to_rfc3339(),
            });
        }
        
        // Must have at least one info block
        if self.info.is_empty() {
            return Err(CapError::MissingField("info".to_string()));
        }
        
        // Validate all info blocks
        for info in &self.info {
            info.validate()?;
        }
        
        // Validate references format if present
        if let Some(ref references) = self.references {
            validate_references(references)?;
        }
        
        Ok(())
    }
}

impl Validate for Info {
    fn validate(&self) -> Result<()> {
        // Event is required
        if self.event.is_empty() {
            return Err(CapError::MissingField("event".to_string()));
        }
        
        // Category is required (at least one)
        if self.category.is_empty() {
            return Err(CapError::MissingField("category".to_string()));
        }
        
        // Validate time relationships
        validate_time_sequence(&self.effective, &self.onset, &self.expires)?;
        
        // Validate language format if present
        if let Some(ref language) = self.language {
            if !is_valid_language_code(language) {
                return Err(CapError::InvalidValue {
                    field: "language".to_string(),
                    value: language.clone(),
                });
            }
        }
        
        // Validate all areas
        for area in &self.area {
            area.validate()?;
        }
        
        // Validate web URL if present
        if let Some(ref web) = self.web {
            if !web.scheme().starts_with("http") {
                return Err(CapError::InvalidValue {
                    field: "web".to_string(),
                    value: web.to_string(),
                });
            }
        }
        
        Ok(())
    }
}

impl Validate for Area {
    fn validate(&self) -> Result<()> {
        // Area description is required
        if self.area_desc.is_empty() {
            return Err(CapError::MissingField("areaDesc".to_string()));
        }
        
        // Must have at least one geographic descriptor
        if self.polygon.is_empty() && self.circle.is_empty() && self.geocode.is_empty() {
            return Err(CapError::ValidationError(
                "Area must have at least one polygon, circle, or geocode".to_string()
            ));
        }
        
        // Validate polygon format
        for polygon in &self.polygon {
            validate_polygon(polygon)?;
        }
        
        // Validate circle format
        for circle in &self.circle {
            validate_circle(circle)?;
        }
        
        // Validate altitude/ceiling relationship
        if let (Some(altitude), Some(ceiling)) = (self.altitude, self.ceiling) {
            if altitude >= ceiling {
                return Err(CapError::ValidationError(
                    "Altitude must be less than ceiling".to_string()
                ));
            }
        }
        
        Ok(())
    }
}

/// Validate sender format (domain-like format)
fn is_valid_sender(sender: &str) -> bool {
    // Basic validation - should contain at least one dot and no spaces
    sender.contains('.') && !sender.contains(' ') && !sender.is_empty()
}

/// Validate language code format (RFC 3066)
fn is_valid_language_code(language: &str) -> bool {
    // Basic validation for language-country format
    let parts: Vec<&str> = language.split('-').collect();
    match parts.len() {
        1 => parts[0].len() == 2 && parts[0].chars().all(|c| c.is_ascii_lowercase()),
        2 => {
            parts[0].len() == 2 
                && parts[0].chars().all(|c| c.is_ascii_lowercase())
                && parts[1].len() == 2
                && parts[1].chars().all(|c| c.is_ascii_uppercase())
        }
        _ => false,
    }
}

/// Validate time sequence (effective <= onset <= expires)
fn validate_time_sequence(
    effective: &Option<DateTime<Utc>>,
    onset: &Option<DateTime<Utc>>,
    expires: &Option<DateTime<Utc>>,
) -> Result<()> {
    // If both effective and onset are present, effective should be <= onset
    if let (Some(effective), Some(onset)) = (effective, onset) {
        if effective > onset {
            return Err(CapError::ValidationError(
                "Effective time must be before or equal to onset time".to_string()
            ));
        }
    }
    
    // If both onset and expires are present, onset should be < expires
    if let (Some(onset), Some(expires)) = (onset, expires) {
        if onset >= expires {
            return Err(CapError::ValidationError(
                "Onset time must be before expires time".to_string()
            ));
        }
    }
    
    // If both effective and expires are present, effective should be < expires
    if let (Some(effective), Some(expires)) = (effective, expires) {
        if effective >= expires {
            return Err(CapError::ValidationError(
                "Effective time must be before expires time".to_string()
            ));
        }
    }
    
    Ok(())
}

/// Validate references format (space-separated sender,identifier,sent triplets)
fn validate_references(references: &str) -> Result<()> {
    if references.trim().is_empty() {
        return Ok(());
    }
    
    // References should be space-separated triplets of sender,identifier,sent
    let triplets: Vec<&str> = references.split_whitespace().collect();
    for triplet in triplets {
        let parts: Vec<&str> = triplet.split(',').collect();
        if parts.len() != 3 {
            return Err(CapError::InvalidValue {
                field: "references".to_string(),
                value: triplet.to_string(),
            });
        }
        
        // Validate the timestamp format
        let timestamp = parts[2];
        if DateTime::parse_from_rfc3339(timestamp).is_err() {
            return Err(CapError::InvalidValue {
                field: "references timestamp".to_string(),
                value: timestamp.to_string(),
            });
        }
    }
    
    Ok(())
}

/// Validate polygon format (space-separated lat,lon pairs, closed polygon)
fn validate_polygon(polygon: &str) -> Result<()> {
    let coordinates: Vec<&str> = polygon.split_whitespace().collect();
    
    if coordinates.len() < 4 {
        return Err(CapError::InvalidValue {
            field: "polygon".to_string(),
            value: "Polygon must have at least 4 coordinate pairs".to_string(),
        });
    }
    
    // Check that first and last coordinate pairs are the same (closed polygon)
    if coordinates.first() != coordinates.last() {
        return Err(CapError::InvalidValue {
            field: "polygon".to_string(),
            value: "Polygon must be closed (first and last coordinates must match)".to_string(),
        });
    }
    
    // Validate each coordinate pair
    for coord in &coordinates {
        validate_coordinate_pair(coord, "polygon")?;
    }
    
    Ok(())
}

/// Validate circle format (lat,lon radius)
fn validate_circle(circle: &str) -> Result<()> {
    let parts: Vec<&str> = circle.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(CapError::InvalidValue {
            field: "circle".to_string(),
            value: "Circle must have coordinate pair and radius".to_string(),
        });
    }
    
    validate_coordinate_pair(parts[0], "circle")?;
    
    // Validate radius is a positive number
    if parts[1].parse::<f64>().is_err() || parts[1].parse::<f64>().unwrap_or(-1.0) <= 0.0 {
        return Err(CapError::InvalidValue {
            field: "circle radius".to_string(),
            value: parts[1].to_string(),
        });
    }
    
    Ok(())
}

/// Validate lat,lon coordinate pair
fn validate_coordinate_pair(coord_pair: &str, field: &str) -> Result<()> {
    let coords: Vec<&str> = coord_pair.split(',').collect();
    if coords.len() != 2 {
        return Err(CapError::InvalidValue {
            field: field.to_string(),
            value: format!("Invalid coordinate pair: {}", coord_pair),
        });
    }
    
    // Validate latitude
    let lat: f64 = coords[0].parse().map_err(|_| CapError::InvalidValue {
        field: format!("{} latitude", field),
        value: coords[0].to_string(),
    })?;
    
    if !(-90.0..=90.0).contains(&lat) {
        return Err(CapError::InvalidValue {
            field: format!("{} latitude", field),
            value: coords[0].to_string(),
        });
    }
    
    // Validate longitude
    let lon: f64 = coords[1].parse().map_err(|_| CapError::InvalidValue {
        field: format!("{} longitude", field),
        value: coords[1].to_string(),
    })?;
    
    if !(-180.0..=180.0).contains(&lon) {
        return Err(CapError::InvalidValue {
            field: format!("{} longitude", field),
            value: coords[1].to_string(),
        });
    }
    
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builder::AlertBuilder, Category, Certainty, Scope, Severity, Urgency};

    #[test]
    fn test_valid_alert() {
        let alert = AlertBuilder::new("example.org")
            .add_info(|info| {
                info.event("Test Event")
                    .urgency(Urgency::Future)
                    .severity(Severity::Minor)
                    .certainty(Certainty::Possible)
                    .add_category(Category::Other)
                    .add_area(|area| {
                        area.area_desc("Test Area")
                            .add_polygon("42.0,-71.0 42.1,-71.0 42.1,-70.9 42.0,-70.9 42.0,-71.0")
                    })
            })
            .build();
        
        assert!(alert.validate().is_ok());
    }
    
    #[test]
    fn test_missing_sender() {
        let mut alert = AlertBuilder::new("example.org").build();
        alert.sender = String::new();
        
        assert!(matches!(alert.validate(), Err(CapError::MissingField(_))));
    }
    
    #[test]
    fn test_invalid_sender() {
        let alert = AlertBuilder::new("invalid sender").build();
        
        assert!(matches!(alert.validate(), Err(CapError::InvalidValue { .. })));
    }
    
    #[test]
    fn test_restricted_scope_requires_restriction() {
        let alert = AlertBuilder::new("example.org")
            .scope(Scope::Restricted)
            .build();
        
        assert!(matches!(alert.validate(), Err(CapError::MissingField(_))));
    }
    
    #[test]
    fn test_language_code_validation() {
        assert!(is_valid_language_code("en"));
        assert!(is_valid_language_code("en-US"));
        assert!(is_valid_language_code("fr-CA"));
        assert!(!is_valid_language_code("english"));
        assert!(!is_valid_language_code("en-us")); // Should be uppercase
        assert!(!is_valid_language_code("EN"));    // Should be lowercase
    }
    
    #[test]
    fn test_coordinate_validation() {
        assert!(validate_coordinate_pair("42.0,-71.0", "test").is_ok());
        assert!(validate_coordinate_pair("0.0,0.0", "test").is_ok());
        assert!(validate_coordinate_pair("90.0,180.0", "test").is_ok());
        assert!(validate_coordinate_pair("-90.0,-180.0", "test").is_ok());
        
        // Invalid formats
        assert!(validate_coordinate_pair("42.0", "test").is_err());
        assert!(validate_coordinate_pair("42.0,-71.0,0.0", "test").is_err());
        assert!(validate_coordinate_pair("invalid,-71.0", "test").is_err());
        
        // Out of range
        assert!(validate_coordinate_pair("91.0,-71.0", "test").is_err());
        assert!(validate_coordinate_pair("42.0,-181.0", "test").is_err());
    }
    
    #[test]
    fn test_polygon_validation() {
        // Valid closed polygon
        assert!(validate_polygon("42.0,-71.0 42.1,-71.0 42.1,-70.9 42.0,-70.9 42.0,-71.0").is_ok());
        
        // Not closed
        assert!(validate_polygon("42.0,-71.0 42.1,-71.0 42.1,-70.9 42.0,-70.9").is_err());
        
        // Too few points
        assert!(validate_polygon("42.0,-71.0 42.1,-71.0 42.0,-71.0").is_err());
    }
    
    #[test]
    fn test_circle_validation() {
        assert!(validate_circle("42.0,-71.0 10.0").is_ok());
        assert!(validate_circle("42.0,-71.0 -10.0").is_err()); // Negative radius
        assert!(validate_circle("42.0,-71.0").is_err());       // Missing radius
    }
}