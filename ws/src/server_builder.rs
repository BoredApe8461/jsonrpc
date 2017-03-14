use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use core;
use server_utils;
use server_utils::cors::Origin;
use server_utils::hosts::DomainsValidation;
use server_utils::reactor::UninitializedRemote;
use ws;

use metadata::{MetaExtractor, NoopExtractor};
use server::Server;
use session;

/// Signer startup error
#[derive(Debug)]
pub enum ServerError {
	/// Wrapped `std::io::Error`
	IoError(io::Error),
	/// Other `ws-rs` error
	WebSocket(ws::Error)
}

impl From<ws::Error> for ServerError {
	fn from(err: ws::Error) -> Self {
		match err.kind {
			ws::ErrorKind::Io(e) => ServerError::IoError(e),
			_ => ServerError::WebSocket(err),
		}
	}
}

impl From<io::Error> for ServerError {
	fn from(err: io::Error) -> Self {
		ServerError::IoError(err)
	}
}

/// Builder for `WebSockets` server
pub struct ServerBuilder<M: core::Metadata, S: core::Middleware<M>> {
	handler: Arc<core::MetaIoHandler<M, S>>,
	meta_extractor: Arc<MetaExtractor<M>>,
	allowed_origins: Option<Vec<Origin>>,
	request_middleware: Option<Arc<session::RequestMiddleware>>,
	session_stats: Option<Arc<session::SessionStats>>,
	remote: UninitializedRemote,
}

impl<M: core::Metadata, S: core::Middleware<M>> ServerBuilder<M, S> {
	/// Creates new `ServerBuilder`
	pub fn new<T>(handler: T) -> Self where
		T: Into<core::MetaIoHandler<M, S>>,
	{
		ServerBuilder {
			handler: Arc::new(handler.into()),
			meta_extractor: Arc::new(NoopExtractor),
			allowed_origins: None,
			request_middleware: None,
			session_stats: None,
			remote: UninitializedRemote::Unspawned,
		}
	}

	/// Utilize existing event loop remote to poll RPC results.
	pub fn event_loop_remote(mut self, remote: server_utils::tokio_core::reactor::Remote) -> Self {
		self.remote = UninitializedRemote::Shared(remote);
		self
	}

	/// Sets a meta extractor.
	pub fn session_meta_extractor<T: MetaExtractor<M>>(mut self, extractor: T) -> Self {
		self.meta_extractor = Arc::new(extractor);
		self
	}

	/// Allowed origins.
	pub fn allowed_origins(mut self, allowed_origins: DomainsValidation<Origin>) -> Self {
		self.allowed_origins = allowed_origins.into();
		self
	}

	/// Session stats
	pub fn session_stats<T: session::SessionStats>(mut self, stats: T) -> Self {
		self.session_stats = Some(Arc::new(stats));
		self
	}

	/// Sets a request middleware. Middleware will be invoked before each handshake request.
	/// You can either terminate the handshake in the middleware or run a default behaviour after.
	pub fn request_middleware<T: session::RequestMiddleware>(mut self, middleware: T) -> Self {
		self.request_middleware = Some(Arc::new(middleware));
		self
	}

	/// Starts a new `WebSocket` server in separate thread.
	/// Returns a `Server` handle which closes the server when droped.
	pub fn start(self, addr: &SocketAddr) -> Result<Server, ServerError> {
		Server::start(
			addr,
			self.handler,
			self.meta_extractor,
			self.allowed_origins,
			self.request_middleware,
			self.session_stats,
			self.remote,
		)
	}

}
