//! The schema's own description, for what this build has no Rust type for.
//!
//! The typed methods cover the arms this build knows. A console, or an agent,
//! also asks for an arm **by name** and sets its fields from text, and both are
//! read from the descriptor set the schema generates: the arms, their numbers,
//! and the message each carries. Nothing here is a second schema; it is the same
//! one, read.

use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, MessageDescriptor, Value as Pb};

use crate::proto;

/// Why a name could not be made into a message.
#[derive(Debug)]
pub enum DynamicError {
    /// The schema's descriptor set did not read.
    Schema(prost_reflect::DescriptorError),
    /// The schema carries no message of that name.
    NoSuchMessage(String),
    /// The schema declares no arm of that name.
    NoSuchArm(String),
    /// The message carries no field of that name.
    NoSuchField(String),
    /// A dotted path ran into a field that is not a message.
    NotAMessage(String),
    /// A field cannot be set from text.
    NotText(String),
    /// The text is not the value the field takes.
    BadValue(String),
    /// A request the schema built did not read back as a request.
    Decode(prost::DecodeError),
}

impl std::fmt::Display for DynamicError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DynamicError::Schema(error) => write!(formatter, "the schema did not read: {error}"),
            DynamicError::NoSuchMessage(name) => {
                write!(formatter, "the schema carries no message named {name}")
            }
            DynamicError::NoSuchArm(arm) => {
                write!(formatter, "the schema declares no arm named {arm}")
            }
            DynamicError::NoSuchField(field) => write!(formatter, "{field} is not a field of it"),
            DynamicError::NotAMessage(field) => {
                write!(formatter, "{field} is not a message, so nothing is in it")
            }
            DynamicError::NotText(kind) => write!(formatter, "a {kind} cannot be set from text"),
            DynamicError::BadValue(text) => write!(formatter, "{text:?} is not that field's value"),
            DynamicError::Decode(error) => write!(formatter, "the request did not read: {error}"),
        }
    }
}

impl std::error::Error for DynamicError {}

impl From<prost_reflect::DescriptorError> for DynamicError {
    fn from(error: prost_reflect::DescriptorError) -> Self {
        DynamicError::Schema(error)
    }
}

impl From<prost::DecodeError> for DynamicError {
    fn from(error: prost::DecodeError) -> Self {
        DynamicError::Decode(error)
    }
}

/// One arm of the request, as the schema declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    /// The name a caller asks by: `sys.ping`.
    pub name: String,
    /// The number the arm travels as.
    pub number: u32,
    /// The message the arm carries: `SysPing`.
    pub carries: String,
}

/// The schema's own description, read.
pub fn pool() -> Result<DescriptorPool, DynamicError> {
    Ok(DescriptorPool::decode(proto::DESCRIPTOR_SET)?)
}

/// The request envelope's description.
pub fn request_of(pool: &DescriptorPool) -> Result<MessageDescriptor, DynamicError> {
    pool.get_message_by_name("ThetaGP.Request")
        .ok_or_else(|| DynamicError::NoSuchMessage("ThetaGP.Request".to_string()))
}

/// Every arm the schema declares, in the schema's own order.
pub fn arms() -> Result<Vec<Arm>, DynamicError> {
    let pool = pool()?;
    let request = request_of(&pool)?;

    Ok(request
        .fields()
        .filter(|field| {
            field
                .containing_oneof()
                .is_some_and(|oneof| oneof.name() == "kind")
        })
        .map(|field| Arm {
            name: arm_name(field.name()),
            number: field.number(),
            carries: match field.kind() {
                Kind::Message(message) => message.name().to_string(),
                other => format!("{other:?}"),
            },
        })
        .collect())
}

/// The dotted name of an arm: the schema spells `sys_ping` as `sys.ping`.
pub fn arm_name(field: &str) -> String {
    match field.split_once('_') {
        Some((domain, rest)) => format!("{domain}.{rest}"),
        None => field.to_string(),
    }
}

/// Builds one request: the arm's own message, with the fields it was given as
/// `field=value`, inside the envelope the wire carries.
///
/// A dotted name walks into a nested message, so `array.elements=1,2` sets the
/// element run of a value that is a run.
pub fn build_request(arm: &str, fields: &[String]) -> Result<proto::Request, DynamicError> {
    let pool = pool()?;
    let request = request_of(&pool)?;
    let field = request
        .get_field_by_name(&arm.replace('.', "_"))
        .ok_or_else(|| DynamicError::NoSuchArm(arm.to_string()))?;

    let Kind::Message(descriptor) = field.kind() else {
        return Err(DynamicError::NotAMessage(arm.to_string()));
    };
    let mut built = DynamicMessage::new(descriptor.clone());

    for assignment in fields {
        let (path, raw) = assignment
            .split_once('=')
            .ok_or_else(|| DynamicError::BadValue(assignment.clone()))?;
        set_field(&mut built, &descriptor, path, raw)?;
    }

    let mut envelope = DynamicMessage::new(request);
    envelope.set_field(&field, Pb::Message(built));

    Ok(proto::Request::decode(envelope.encode_to_vec().as_slice())?)
}

/// Sets one field of a message described by `descriptor`, walking a dotted path
/// into a nested message.
pub fn set_field(
    message: &mut DynamicMessage,
    descriptor: &MessageDescriptor,
    path: &str,
    raw: &str,
) -> Result<(), DynamicError> {
    let (head, rest) = match path.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (path, None),
    };
    let field = match descriptor.get_field_by_name(head) {
        Some(field) => field,
        // A oneof's own name is a step of the path as well: the schema spells
        // `value.array.elements`, where `value` is the group the two shapes of a
        // value belong to and `array` is the one that is in it.
        None if descriptor.oneofs().any(|oneof| oneof.name() == head) => {
            let Some(rest) = rest else {
                return Err(DynamicError::NoSuchField(head.to_string()));
            };
            return set_field(message, descriptor, rest, raw);
        }
        None => return Err(DynamicError::NoSuchField(head.to_string())),
    };

    match rest {
        None => {
            let value = coerce(&field, raw)?;
            message.set_field(&field, value);
            Ok(())
        }
        Some(rest) => {
            let Kind::Message(nested_descriptor) = field.kind() else {
                return Err(DynamicError::NotAMessage(head.to_string()));
            };
            let mut nested = match &*message.get_field(&field) {
                Pb::Message(nested) => nested.clone(),
                _ => DynamicMessage::new(nested_descriptor.clone()),
            };
            set_field(&mut nested, &nested_descriptor, rest, raw)?;
            message.set_field(&field, Pb::Message(nested));
            Ok(())
        }
    }
}

/// The value a field takes from the text it was given.
///
/// A field that repeats takes the text as a run, one element per comma.
pub fn coerce(field: &prost_reflect::FieldDescriptor, raw: &str) -> Result<Pb, DynamicError> {
    if field.is_list() {
        let mut elements = Vec::new();
        for piece in raw.split(',') {
            elements.push(coerce_one(field, piece.trim())?);
        }
        return Ok(Pb::List(elements));
    }
    coerce_one(field, raw)
}

/// The value one field of a message takes from the text it was given.
fn coerce_one(field: &prost_reflect::FieldDescriptor, raw: &str) -> Result<Pb, DynamicError> {
    let bad = || DynamicError::BadValue(raw.to_string());
    Ok(match field.kind() {
        Kind::Bool => Pb::Bool(match raw {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => return Err(bad()),
        }),
        Kind::Int32 | Kind::Sint32 | Kind::Sfixed32 => Pb::I32(raw.parse().map_err(|_| bad())?),
        Kind::Uint32 | Kind::Fixed32 => Pb::U32(raw.parse().map_err(|_| bad())?),
        Kind::Int64 | Kind::Sint64 | Kind::Sfixed64 => Pb::I64(raw.parse().map_err(|_| bad())?),
        Kind::Uint64 | Kind::Fixed64 => Pb::U64(raw.parse().map_err(|_| bad())?),
        Kind::Float => Pb::F32(raw.parse().map_err(|_| bad())?),
        Kind::Double => Pb::F64(raw.parse().map_err(|_| bad())?),
        Kind::String => Pb::String(raw.to_string()),
        Kind::Bytes => Pb::Bytes(parse_hex(raw)?.into()),
        Kind::Enum(descriptor) => match descriptor.get_value_by_name(raw) {
            Some(value) => Pb::EnumNumber(value.number()),
            None => Pb::EnumNumber(raw.parse().map_err(|_| bad())?),
        },
        other => return Err(DynamicError::NotText(format!("{other:?}"))),
    })
}

/// The bytes a hex string carries; spaces, colons and underscores may separate
/// the digits.
pub fn parse_hex(text: &str) -> Result<Vec<u8>, DynamicError> {
    let digits: String = text
        .chars()
        .filter(|character| !character.is_whitespace() && *character != ':' && *character != '_')
        .collect();
    if digits.len() % 2 != 0 {
        return Err(DynamicError::BadValue(format!(
            "{text:?} carries an odd number of hex digits"
        )));
    }

    let mut bytes = Vec::with_capacity(digits.len() / 2);
    for pair in digits.as_bytes().chunks(2) {
        let pair =
            std::str::from_utf8(pair).map_err(|_| DynamicError::BadValue(text.to_string()))?;
        bytes.push(
            u8::from_str_radix(pair, 16).map_err(|_| DynamicError::BadValue(pair.to_string()))?,
        );
    }
    Ok(bytes)
}

/// The bytes as the hex a report spells them with.
pub fn hex_of(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A payload as the message it is, as the text a report prints.
///
/// A payload is a request or a reply and carries nothing that says which, so
/// both are tried and the one that reads as a message with an arm is handed
/// back; a payload that reads as neither is its own bytes.
pub fn describe(payload: &[u8]) -> String {
    if let Ok(request) = proto::Request::decode(payload) {
        if request.kind.is_some() {
            return format!("a request: {request:?}");
        }
    }
    if let Ok(reply) = proto::Reply::decode(payload) {
        if reply.kind.is_some() {
            return format!("a reply: {reply:?}");
        }
    }
    hex_of(payload)
}
