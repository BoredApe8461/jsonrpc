//! jsonrpc io
use std::sync::Arc;
use std::collections::HashMap;
use async::{AsyncResult, Ready};
use super::{MethodCommand, AsyncMethodCommand, AsyncMethod, MethodResult, NotificationCommand, Params, Value, Error, SyncResponse, Version, Id, ErrorCode, RequestHandler, Request, Failure, Response};
use serde_json;

struct DelegateMethod<T, F> where
	F: Fn(&T, Params) -> Result<Value, Error>,
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F
}

impl<T, F> MethodCommand for DelegateMethod <T, F> where
	F: Fn(&T, Params) -> Result<Value, Error>,
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&self, params: Params) -> MethodResult {
		let closure = &self.closure;
		MethodResult::Sync(closure(&self.delegate, params))
	}
}

struct DelegateAsyncMethod<T, F> where
	F: Fn(&T, Params, Ready),
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F
}

impl<T, F> MethodCommand for DelegateAsyncMethod <T, F> where
	F: Fn(&T, Params, Ready),
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&self, params: Params) -> MethodResult {
		let (res, ready) = AsyncResult::new();
		let closure = &self.closure;
		closure(&self.delegate, params, ready);
		res.into()
	}
}

struct DelegateNotification<T, F> where
	F: Fn(&T, Params),
	F: Send + Sync,
	T: Send + Sync {
	delegate: Arc<T>,
	closure: F
}

impl<T, F> NotificationCommand for DelegateNotification<T, F> where
	F: Fn(&T, Params),
	F: Send + Sync,
	T: Send + Sync {
	fn execute(&self, params: Params) {
		let closure = &self.closure;
		closure(&self.delegate, params)
	}
}

/// A set of RPC methods and notifications tied to single `delegate` struct.
pub struct IoDelegate<T> where T: Send + Sync + 'static {
	delegate: Arc<T>,
	methods: HashMap<String, Box<MethodCommand>>,
	notifications: HashMap<String, Box<NotificationCommand>>
}

impl<T> IoDelegate<T> where T: Send + Sync + 'static {
	/// Creates new `IoDelegate`
	pub fn new(delegate: Arc<T>) -> Self {
		IoDelegate {
			delegate: delegate,
			methods: HashMap::new(),
			notifications: HashMap::new()
		}
	}

	/// Add new supported method
	pub fn add_method<F>(&mut self, name: &str, closure: F) where F: Fn(&T, Params) -> Result<Value, Error> + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.methods.insert(name.to_owned(), Box::new(DelegateMethod {
			delegate: delegate,
			closure: closure
		}));
	}

	/// Add new supported asynchronous method
	pub fn add_async_method<F>(&mut self, name: &str, closure: F) where F: Fn(&T, Params, Ready) + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.methods.insert(name.to_owned(), Box::new(DelegateAsyncMethod {
			delegate: delegate,
			closure: closure
		}));
	}

	/// Add new supported notification
	pub fn add_notification<F>(&mut self, name: &str, closure: F) where F: Fn(&T, Params) + Send + Sync + 'static {
		let delegate = self.delegate.clone();
		self.notifications.insert(name.to_owned(), Box::new(DelegateNotification {
			delegate: delegate,
			closure: closure
		}));
	}
}

/// Asynchronous string response
#[derive(Debug)]
pub struct AsyncStringResponse {
	response: Response,
}

impl AsyncStringResponse {
	/// wraps sync response
	fn wrap(res: SyncResponse) -> String {
		let response = write_response(res);
		debug!(target: "rpc", "Response: {:?}", response);
		response
	}

	/// Adds closure to be invoked when result is available.
	/// Callback is invoked right away if result is instantly available and `true` is returned.
	/// `false` is returned when listener has been added
	pub fn on_result<F>(self, f: F) -> bool where F: FnOnce(String) + Send + 'static {
		self.response.on_result(move |res| {
			f(Self::wrap(res))
		})
	}

	/// Blocks current thread and awaits for a result.
	pub fn await(self) -> String {
		Self::wrap(self.response.await())
	}
}

impl From<Response> for AsyncStringResponse {
	fn from(response: Response) -> Self {
		AsyncStringResponse {
			response: response,
		}
	}
}


/// Should be used to handle jsonrpc io.
///
/// ```rust
/// extern crate jsonrpc_core;
/// use jsonrpc_core::*;
///
/// fn main() {
/// 	let io = IoHandler::new();
/// 	struct SayHello;
///
/// 	// Implement `SyncMethodCommand` or `AsyncMethodCommand`
/// 	impl SyncMethodCommand for SayHello {
/// 		fn execute(&self, _params: Params) -> Result<Value, Error>  {
/// 			Ok(Value::String("hello".to_string()))
/// 		}
/// 	}
///
/// 	io.add_method("say_hello", SayHello);
/// 	// Or just use closures
/// 	io.add_async_method("say_hello_async", |_params: Params, ready: Ready| {
///			ready.ready(Ok(Value::String("hello".to_string())))
/// 	});
///
/// 	let request1 = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
/// 	let request2 = r#"{"jsonrpc": "2.0", "method": "say_hello_async", "params": [42, 23], "id": 1}"#;
/// 	let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;
///
/// 	assert_eq!(io.handle_request_sync(request1), Some(response.to_string()));
/// 	assert_eq!(io.handle_request_sync(request2), Some(response.to_string()));
/// }
/// ```
pub struct IoHandler {
	request_handler: RequestHandler
}

fn read_request(request_str: &str) -> Result<Request, Error> {
	serde_json::from_str(request_str).map_err(|_| Error::new(ErrorCode::ParseError))
}

fn write_response(response: SyncResponse) -> String {
	// this should never fail
	serde_json::to_string(&response).unwrap()
}

impl IoHandler {
	/// Creates new `IoHandler`
	pub fn new() -> Self {
		IoHandler {
			request_handler: RequestHandler::new()
		}
	}

	/// Add supported method
	pub fn add_method<C>(&self, name: &str, command: C) where C: MethodCommand + 'static {
		self.request_handler.add_method(name.to_owned(), Box::new(command))
	}

	/// Add supported asynchronous method
	pub fn add_async_method<C>(&self, name: &str, command: C) where C: AsyncMethodCommand + 'static {
		self.request_handler.add_method(name.to_owned(), Box::new(AsyncMethod::new(command)))
	}

	/// Add supported notification
	pub fn add_notification<C>(&self, name: &str, command: C) where C: NotificationCommand + 'static {
		self.request_handler.add_notification(name.to_owned(), Box::new(command))
	}

	/// Add delegate with supported methods.
	pub fn add_delegate<D>(&self, delegate: IoDelegate<D>) where D: Send + Sync {
		self.request_handler.add_methods(delegate.methods);
		self.request_handler.add_notifications(delegate.notifications);
	}

	/// Handle given request synchronously - will block until response is available.
	/// If you have any asynchronous methods in your RPC it is much wiser to use
	/// `handle_request` instead and deal with asynchronous requests in a non-blocking fashion.
	pub fn handle_request_sync<'a>(&self, request_str: &'a str) -> Option<String> {
		self.handle_request(request_str).map(|res| res.await())
	}

	/// Handle given request asynchronously.
	pub fn handle_request<'a>(&self, request_str: &'a str) -> Option<AsyncStringResponse> {
		trace!(target: "rpc", "Request: {}", request_str);
		let response = match read_request(request_str) {
			Ok(request) => self.request_handler.handle_request(request).map(AsyncStringResponse::from),
			Err(error) => Some(Response::from(Failure {
				id: Id::Null,
				jsonrpc: Version::V2,
				error: error
			}).into()),
		};
		trace!(target: "rpc", "AsyncResponse: {:?}", response);
		response
	}
}

#[cfg(test)]
mod tests {
	use std::time::Duration;
	use std::thread;
	use super::super::*;

	#[test]
	fn test_io_handler() {
		let io = IoHandler::new();

		struct SayHello;
		impl SyncMethodCommand for SayHello {
			fn execute(&self, _params: Params) -> Result<Value, Error> {
				Ok(Value::String("hello".to_string()))
			}
		}

		io.add_method("say_hello", SayHello);

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler() {
		let io = IoHandler::new();

		struct SayHelloAsync;
		impl AsyncMethodCommand for SayHelloAsync {
			fn execute(&self, _params: Params, ready: Ready) {
				ready.ready(Ok(Value::String("hello".to_string())))
			}
		}

		io.add_async_method("say_hello", SayHelloAsync);

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

	#[test]
	fn test_async_io_handler_thread() {
		let io = IoHandler::new();

		struct SayHelloAsync;
		impl AsyncMethodCommand for SayHelloAsync {
			fn execute(&self, _params: Params, ready: Ready) {
				thread::spawn(|| {
					thread::sleep(Duration::from_secs(1));
					ready.ready(Ok(Value::String("hello".to_string())))
				});
			}
		}

		io.add_async_method("say_hello", SayHelloAsync);

		let request = r#"{"jsonrpc": "2.0", "method": "say_hello", "params": [42, 23], "id": 1}"#;
		let response = r#"{"jsonrpc":"2.0","result":"hello","id":1}"#;

		assert_eq!(io.handle_request_sync(request), Some(response.to_string()));
	}

}
