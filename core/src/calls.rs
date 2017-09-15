use std::sync::Arc;
use types::{Params, Value, Error};
use futures::{Future, IntoFuture};
use BoxFuture;

/// Metadata trait
pub trait Metadata: Default + Clone + Send + 'static {}
impl Metadata for () {}

/// Asynchronous Method
pub trait RpcMethodSimple: Send + Sync + 'static {
	/// Output future
	type Out: Future<Item = Value, Error = Error> + Send;
	/// Call method
	fn call(&self, params: Params) -> Self::Out;
}

/// Asynchronous Method with Metadata
pub trait RpcMethod<T: Metadata>: Send + Sync + 'static {
	/// Call method
	fn call(&self, params: Params, meta: T) -> BoxFuture<Value, Error>;
}

/// Notification
pub trait RpcNotificationSimple: Send + Sync + 'static {
	/// Execute notification
	fn execute(&self, params: Params);
}

/// Notification with Metadata
pub trait RpcNotification<T: Metadata>: Send + Sync + 'static {
	/// Execute notification
	fn execute(&self, params: Params, meta: T);
}

/// Possible Remote Procedures with Metadata
#[derive(Clone)]
pub enum RemoteProcedure<T: Metadata> {
	/// A method call
	Method(Arc<RpcMethod<T>>),
	/// A notification
	Notification(Arc<RpcNotification<T>>),
	/// An alias to other method,
	Alias(String),
}

impl<F: Send + Sync + 'static, X: Send + 'static, I> RpcMethodSimple for F where
	F: Fn(Params) -> I,
	X: Future<Item = Value, Error = Error>,
	I: IntoFuture<Item = Value, Error = Error, Future = X>,
{
	type Out = X;
	fn call(&self, params: Params) -> Self::Out {
		self(params).into_future()
	}
}

impl<F: Send + Sync + 'static> RpcNotificationSimple for F where
	F: Fn(Params),
{
	fn execute(&self, params: Params) {
		self(params)
	}
}

impl<F: Send + Sync + 'static, X: Send + 'static, T, I> RpcMethod<T> for F where
	T: Metadata,
	F: Fn(Params, T) -> I,
	I: IntoFuture<Item = Value, Error = Error, Future = X>,
	X: Future<Item = Value, Error = Error>,
{
	fn call(&self, params: Params, meta: T) -> BoxFuture<Value, Error> {
		Box::new(self(params, meta).into_future())
	}
}

impl<F: Send + Sync + 'static, T> RpcNotification<T> for F where
	T: Metadata,
	F: Fn(Params, T),
{
	fn execute(&self, params: Params, meta: T) {
		self(params, meta)
	}
}
