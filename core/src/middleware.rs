//! `IoHandler` middlewares

use calls::Metadata;
use types::{Request, Response};
use futures::{future::Either, Future};

/// RPC middleware
pub trait Middleware<M: Metadata>: Send + Sync + 'static {
	/// A returned future.
	type Future: Future<Item=Option<Response>, Error=()> + Send + 'static;

	/// Method invoked on each request.
	/// Allows you to either respond directly (without executing RPC call)
	/// or do any additional work before and/or after processing the request.
	fn on_request<F, X>(&self, request: Request, meta: M, next: F) -> Either<Self::Future, X> where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static;
}

/// No-op middleware implementation
#[derive(Debug, Default)]
pub struct Noop;
impl<M: Metadata> Middleware<M> for Noop {
	type Future = Box<Future<Item=Option<Response>, Error=()> + Send>;

	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> Either<Self::Future, X> where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		Either::B(process(request, meta))
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>>
	Middleware<M> for (A, B)
{
	type Future = Either<A::Future, B::Future>;

	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> Either<Self::Future, X> where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		repack(self.0.on_request(request, meta, move |request, meta| {
			self.1.on_request(request, meta, process)
		}))
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>, C: Middleware<M>>
	Middleware<M> for (A, B, C)
{
	type Future = Either<A::Future, Either<B::Future, C::Future>>;

	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> Either<Self::Future, X> where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		repack(self.0.on_request(request, meta, move |request, meta| {
			repack(self.1.on_request(request, meta, move |request, meta| {
				self.2.on_request(request, meta, process)
			}))
		}))
	}
}

impl<M: Metadata, A: Middleware<M>, B: Middleware<M>, C: Middleware<M>, D: Middleware<M>>
	Middleware<M> for (A, B, C, D)
{
	type Future = Either<A::Future, Either<B::Future, Either<C::Future, D::Future>>>;

	fn on_request<F, X>(&self, request: Request, meta: M, process: F) -> Either<Self::Future, X> where
		F: FnOnce(Request, M) -> X + Send,
		X: Future<Item=Option<Response>, Error=()> + Send + 'static,
	{
		repack(self.0.on_request(request, meta, move |request, meta| {
			repack(self.1.on_request(request, meta, move |request, meta| {
				repack(self.2.on_request(request, meta, move |request, meta| {
					self.3.on_request(request, meta, process)
				}))
			}))
		}))
	}
}

#[inline(always)]
fn repack<A, B, X>(result: Either<A, Either<B, X>>) -> Either<Either<A, B>, X> {
	match result {
		Either::A(a) => Either::A(Either::A(a)),
		Either::B(Either::A(b)) => Either::A(Either::B(b)),
		Either::B(Either::B(x)) => Either::B(x),
	}
}
