//! ABOUTME: CAP (Common Alerting Protocol) message builder and validator
//! ABOUTME: Creates standardized emergency alert messages

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

pub mod builder;
pub mod profiles;
pub mod validation;

#[cfg(feature = "xsd-validation")]
pub mod xsd;

/// Result type for CAP operations
pub type Result<T> = std::result::Result<T, CapError>;

/// Errors that can occur during CAP operations
#[derive(Error, Debug)]
pub enum CapError {
    #[error("XML serialization error: {0}")]
    XmlSerializationError(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
    #[error("Invalid value for field {field}: {value}")]
    InvalidValue { field: String, value: String },
    #[error("URL parse error: {0}")]
    UrlError(#[from] url::ParseError),
    #[cfg(feature = "xsd-validation")]
    #[error("XSD validation error: {0}")]
    XsdValidationError(String),
}

/// CAP Alert status values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Status {
    Actual,
    Exercise,
    System,
    Test,
    Draft,
}

/// CAP Alert message type values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum MsgType {
    Alert,
    Update,
    Cancel,
    Ack,
    Error,
}

/// CAP Alert scope values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Scope {
    Public,
    Restricted,
    Private,
}

/// CAP Info category values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Category {
    Geo,
    Met,
    Safety,
    Security,
    Rescue,
    Fire,
    Health,
    Env,
    Transport,
    Infra,
    CBRNE,
    Other,
}

/// CAP Info urgency values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Urgency {
    Immediate,
    Expected,
    Future,
    Past,
    Unknown,
}

/// CAP Info severity values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Severity {
    Extreme,
    Severe,
    Moderate,
    Minor,
    Unknown,
}

/// CAP Info certainty values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Certainty {
    Observed,
    Likely,
    Possible,
    Unlikely,
    Unknown,
}

/// CAP Info response type values
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum ResponseType {
    Shelter,
    Evacuate,
    Prepare,
    Execute,
    Avoid,
    Monitor,
    Assess,
    AllClear,
    None,
}

/// Main CAP Alert message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename = "alert")]
pub struct Alert {
    /// CAP namespace
    #[serde(rename = "@xmlns")]
    pub xmlns: String,

    /// Alert identifier
    pub identifier: String,

    /// Alert sender ID
    pub sender: String,

    /// Timestamp when alert was sent
    pub sent: DateTime<Utc>,

    /// Alert status
    pub status: Status,

    /// Message type
    #[serde(rename = "msgType")]
    pub msg_type: MsgType,

    /// Alert scope
    pub scope: Scope,

    /// Optional source of alert
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Optional restriction for restricted scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restriction: Option<String>,

    /// Optional addresses for private scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addresses: Option<String>,

    /// Optional alert handling codes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Vec<String>>,

    /// Optional note
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,

    /// Optional references to other alerts
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<String>,

    /// Optional incidents this alert relates to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incidents: Option<String>,

    /// Alert information blocks
    pub info: Vec<Info>,
}

/// Wrapper for Category to handle XML serialization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategoryWrapper {
    #[serde(rename = "$text")]
    pub category: Category,
}

impl From<Category> for CategoryWrapper {
    fn from(category: Category) -> Self {
        Self { category }
    }
}

impl From<CategoryWrapper> for Category {
    fn from(wrapper: CategoryWrapper) -> Self {
        wrapper.category
    }
}

/// Wrapper for ResponseType to handle XML serialization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResponseTypeWrapper {
    #[serde(rename = "$text")]
    pub response_type: ResponseType,
}

impl From<ResponseType> for ResponseTypeWrapper {
    fn from(response_type: ResponseType) -> Self {
        Self { response_type }
    }
}

impl From<ResponseTypeWrapper> for ResponseType {
    fn from(wrapper: ResponseTypeWrapper) -> Self {
        wrapper.response_type
    }
}

/// CAP Info block containing alert details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    /// Language code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,

    /// Alert categories
    #[serde(rename = "category")]
    pub category: Vec<CategoryWrapper>,

    /// Event type
    pub event: String,

    /// Response types
    #[serde(
        rename = "responseType",
        skip_serializing_if = "Vec::is_empty",
        default
    )]
    pub response_type: Vec<ResponseTypeWrapper>,

    /// Alert urgency
    pub urgency: Urgency,

    /// Alert severity
    pub severity: Severity,

    /// Alert certainty
    pub certainty: Certainty,

    /// Audience description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,

    /// Event codes
    #[serde(rename = "eventCode", skip_serializing_if = "Vec::is_empty", default)]
    pub event_code: Vec<EventCode>,

    /// Effective time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective: Option<DateTime<Utc>>,

    /// Onset time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onset: Option<DateTime<Utc>>,

    /// Expires time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<DateTime<Utc>>,

    /// Sender name
    #[serde(rename = "senderName", skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,

    /// Headline
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Instructions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,

    /// Web resource
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<Url>,

    /// Contact information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,

    /// Parameters
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub parameter: Vec<Parameter>,

    /// Resources
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub resource: Vec<Resource>,

    /// Areas
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub area: Vec<Area>,
}

/// CAP Event Code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventCode {
    #[serde(rename = "valueName")]
    pub value_name: String,
    pub value: String,
}

/// CAP Parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Parameter {
    #[serde(rename = "valueName")]
    pub value_name: String,
    pub value: String,
}

/// CAP Resource (media attachments)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    /// Resource description
    #[serde(rename = "resourceDesc")]
    pub resource_desc: String,

    /// MIME type
    #[serde(rename = "mimeType")]
    pub mime_type: String,

    /// File size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,

    /// Resource URI
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<Url>,

    /// Dereferenced URI
    #[serde(rename = "derefUri", skip_serializing_if = "Option::is_none")]
    pub deref_uri: Option<String>,

    /// Digest (hash)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

/// CAP Area (geographic region)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    /// Area description
    #[serde(rename = "areaDesc")]
    pub area_desc: String,

    /// Polygon coordinates
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub polygon: Vec<String>,

    /// Circle coordinates
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub circle: Vec<String>,

    /// Geocodes
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub geocode: Vec<Geocode>,

    /// Altitude
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude: Option<f64>,

    /// Ceiling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<f64>,
}

/// CAP Geocode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geocode {
    #[serde(rename = "valueName")]
    pub value_name: String,
    pub value: String,
}

impl Alert {
    /// Create a new CAP Alert with default namespace
    pub fn new(identifier: String, sender: String) -> Self {
        Self {
            xmlns: "urn:oasis:names:tc:emergency:cap:1.2".to_string(),
            identifier,
            sender,
            sent: Utc::now(),
            status: Status::Actual,
            msg_type: MsgType::Alert,
            scope: Scope::Public,
            source: None,
            restriction: None,
            addresses: None,
            code: None,
            note: None,
            references: None,
            incidents: None,
            info: Vec::new(),
        }
    }

    /// Add an Info block to the alert
    pub fn add_info(mut self, info: Info) -> Self {
        self.info.push(info);
        self
    }

    /// Serialize to XML string
    pub fn to_xml(&self) -> Result<String> {
        quick_xml::se::to_string(self)
            .map_err(|e| CapError::XmlSerializationError(format!("Serialization error: {}", e)))
    }

    /// Deserialize from XML string
    pub fn from_xml(xml: &str) -> Result<Self> {
        quick_xml::de::from_str(xml)
            .map_err(|e| CapError::XmlSerializationError(format!("Deserialization error: {}", e)))
    }
}

impl Info {
    /// Create a new Info block
    pub fn new(event: String, urgency: Urgency, severity: Severity, certainty: Certainty) -> Self {
        Self {
            language: Some("en-US".to_string()),
            category: vec![CategoryWrapper::from(Category::Other)],
            event,
            response_type: Vec::new(),
            urgency,
            severity,
            certainty,
            audience: None,
            event_code: Vec::new(),
            effective: None,
            onset: None,
            expires: None,
            sender_name: None,
            headline: None,
            description: None,
            instruction: None,
            web: None,
            contact: None,
            parameter: Vec::new(),
            resource: Vec::new(),
            area: Vec::new(),
        }
    }

    /// Add a category to the info
    pub fn add_category(mut self, category: Category) -> Self {
        let wrapper = CategoryWrapper::from(category);
        if !self.category.iter().any(|c| c.category == wrapper.category) {
            self.category.push(wrapper);
        }
        self
    }

    /// Set the headline
    pub fn headline(mut self, headline: String) -> Self {
        self.headline = Some(headline);
        self
    }

    /// Set the description
    pub fn description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Set the instruction
    pub fn instruction(mut self, instruction: String) -> Self {
        self.instruction = Some(instruction);
        self
    }

    /// Add an area to the info
    pub fn add_area(mut self, area: Area) -> Self {
        self.area.push(area);
        self
    }
}
