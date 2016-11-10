//! jsonrpc request
use serde::de::{Deserialize, Deserializer, Error as DeError};
use serde::ser::{Serialize, Serializer, Error as SerError};
use serde_json::{value, Error as JsonError};
use super::{Id, Params, Version, Value};

/// Represents jsonrpc request which is a method call.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MethodCall {
	/// A String specifying the version of the JSON-RPC protocol.
	/// MUST be exactly "2.0".
	pub jsonrpc: Version,
	/// A String containing the name of the method to be invoked.
	pub method: String,
	/// A Structured value that holds the parameter values to be used
	/// during the invocation of the method. This member MAY be omitted.
	pub params: Option<Params>,
	/// An identifier established by the Client that MUST contain a String,
	/// Number, or NULL value if included. If it is not included it is assumed
	/// to be a notification.
	pub id: Id,
}

/// Represents jsonrpc request which is a notification.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Notification {
	/// A String specifying the version of the JSON-RPC protocol.
	/// MUST be exactly "2.0".
	pub jsonrpc: Version,
	/// A String containing the name of the method to be invoked.
	pub method: String,
	/// A Structured value that holds the parameter values to be used
	/// during the invocation of the method. This member MAY be omitted.
	pub params: Option<Params>
}

/// Represents single jsonrpc call.
#[derive(Debug, PartialEq)]
pub enum Call {
	MethodCall(MethodCall),
	Notification(Notification),
	Invalid(Id),
}

impl Serialize for Call {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error>
	where S: Serializer {
		match *self {
			Call::MethodCall(ref m) => m.serialize(serializer),
			Call::Notification(ref n) => n.serialize(serializer),
			Call::Invalid(_) => Err(S::Error::custom("invalid call"))
		}
	}
}

impl Deserialize for Call {
	fn deserialize<D>(deserializer: &mut D) -> Result<Call, D::Error>
	where D: Deserializer {
		let v = try!(Value::deserialize(deserializer));
		value::from_value(v.clone()).map(Call::Notification)
			.or_else(|_: JsonError| value::from_value(v.clone()).map(Call::MethodCall))
			.or_else(|_: JsonError| {
				let id = v.find("id")
					.and_then(|id| value::from_value(id.clone()).ok())
					.unwrap_or(Id::Null);
				Ok(Call::Invalid(id))
			})
			.map_err(|_: JsonError| D::Error::custom("")) // make the types match
	}
}

/// Represents jsonrpc request.
#[derive(Debug, PartialEq)]
pub enum Request {
	Single(Call),
	Batch(Vec<Call>)
}

impl Serialize for Request {
	fn serialize<S>(&self, serializer: &mut S) -> Result<(), S::Error>
	where S: Serializer {
		match * self {
			Request::Single(ref call) => call.serialize(serializer),
			Request::Batch(ref calls) => calls.serialize(serializer),
		}
	}
}

impl Deserialize for Request {
	fn deserialize<D>(deserializer: &mut D) -> Result<Request, D::Error>
	where D: Deserializer {
		let v = try!(Value::deserialize(deserializer));
		Deserialize::deserialize(&mut value::Deserializer::new(v.clone())).map(Request::Batch)
			.or_else(|_| Deserialize::deserialize(&mut value::Deserializer::new(v.clone())).map(Request::Single))
			.map_err(|_| D::Error::custom("")) // unreachable, but types must match
	}
}

#[test]
fn method_call_serialize() {
	use serde_json;
	use serde_json::Value;

	let m = MethodCall {
		jsonrpc: Version::V2,
		method: "update".to_owned(),
		params: Some(Params::Array(vec![Value::U64(1), Value::U64(2)])),
		id: Id::Num(1)
	};

	let serialized = serde_json::to_string(&m).unwrap();
	assert_eq!(serialized, r#"{"jsonrpc":"2.0","method":"update","params":[1,2],"id":1}"#);
}

#[test]
fn notification_serialize() {
	use serde_json;
	use serde_json::Value;

	let n = Notification {
		jsonrpc: Version::V2,
		method: "update".to_owned(),
		params: Some(Params::Array(vec![Value::U64(1), Value::U64(2)]))
	};

	let serialized = serde_json::to_string(&n).unwrap();
	assert_eq!(serialized, r#"{"jsonrpc":"2.0","method":"update","params":[1,2]}"#);
}

#[test]
fn call_serialize() {
	use serde_json;
	use serde_json::Value;

	let n = Call::Notification(Notification {
		jsonrpc: Version::V2,
		method: "update".to_owned(),
		params: Some(Params::Array(vec![Value::U64(1)]))
	});

	let serialized = serde_json::to_string(&n).unwrap();
	assert_eq!(serialized, r#"{"jsonrpc":"2.0","method":"update","params":[1]}"#);
}

#[test]
fn request_serialize_batch() {
	use serde_json;

	let batch = Request::Batch(vec![
		Call::MethodCall(MethodCall {
			jsonrpc: Version::V2,
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::U64(1), Value::U64(2)])),
			id: Id::Num(1)
		}),
		Call::Notification(Notification {
			jsonrpc: Version::V2,
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::U64(1)]))
		})
	]);

	let serialized = serde_json::to_string(&batch).unwrap();
	assert_eq!(serialized, r#"[{"jsonrpc":"2.0","method":"update","params":[1,2],"id":1},{"jsonrpc":"2.0","method":"update","params":[1]}]"#);

}

#[test]
fn notification_deserialize() {
	use serde_json;
	use serde_json::Value;

	let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1,2]}"#;
	let deserialized: Notification = serde_json::from_str(s).unwrap();

	assert_eq!(deserialized, Notification {
		jsonrpc: Version::V2,
		method: "update".to_owned(),
		params: Some(Params::Array(vec![Value::U64(1), Value::U64(2)]))
	});

	let s = r#"{"jsonrpc": "2.0", "method": "foobar"}"#;
	let deserialized: Notification = serde_json::from_str(s).unwrap();

	assert_eq!(deserialized, Notification {
		jsonrpc: Version::V2,
		method: "foobar".to_owned(),
		params: None
	});

	let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1,2], "id": 1}"#;
	let deserialized: Result<Notification, _> = serde_json::from_str(s);
	assert!(deserialized.is_err())
}

#[test]
fn call_deserialize() {
	use serde_json;

	let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1]}"#;
	let deserialized: Call = serde_json::from_str(s).unwrap();
	assert_eq!(deserialized, Call::Notification(Notification {
		jsonrpc: Version::V2,
		method: "update".to_owned(),
		params: Some(Params::Array(vec![Value::U64(1)]))
	}));

	let s = r#"{"jsonrpc": "2.0", "method": "update", "params": [1], "id": 1}"#;
	let deserialized: Call = serde_json::from_str(s).unwrap();
	assert_eq!(deserialized, Call::MethodCall(MethodCall {
		jsonrpc: Version::V2,
		method: "update".to_owned(),
		params: Some(Params::Array(vec![Value::U64(1)])),
		id: Id::Num(1)
	}));
}

#[test]
fn request_deserialize_batch() {
	use serde_json;

	let s = r#"[1, {"jsonrpc": "2.0", "method": "update", "params": [1,2], "id": 1},{"jsonrpc": "2.0", "method": "update", "params": [1]}]"#;
	let deserialized: Request = serde_json::from_str(s).unwrap();
	assert_eq!(deserialized, Request::Batch(vec![
		Call::Invalid(Id::Null),
		Call::MethodCall(MethodCall {
			jsonrpc: Version::V2,
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::U64(1), Value::U64(2)])),
			id: Id::Num(1)
		}),
		Call::Notification(Notification {
			jsonrpc: Version::V2,
			method: "update".to_owned(),
			params: Some(Params::Array(vec![Value::U64(1)]))
		})
	]))
}

#[test]
fn request_invalid_returns_id() {
	use serde_json;

	let s = r#"{"id":120,"method":"my_method","params":["foo", "bar"]}"#;
	let deserialized: Request = serde_json::from_str(s).unwrap();
	match deserialized {
		Request::Single(Call::Invalid(Id::Num(120))) => {},
		_ => panic!("Request wrongly deserialized: {:?}", deserialized),
	}
}
