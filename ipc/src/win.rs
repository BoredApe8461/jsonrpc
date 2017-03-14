// Copyright 2015, 2016 Ethcore (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

//! jsonrpc server over win named pipes
//!
//! ```no_run
//! extern crate jsonrpc_core;
//! extern crate jsonrpc_ipc_server;
//!
//! use jsonrpc_core::*;
//! use jsonrpc_ipc_server::Server;
//!
//! fn main() {
//! 	let mut io = IoHandler::new();
//! 	io.add_method("say_hello", |_params| {
//!			Ok(Value::String("hello".into()))
//!		});
//! 	let server = Server::new("/tmp/json-ipc-test.ipc", io).unwrap();
//!     ::std::thread::spawn(move || server.run());
//! }
//! ```

//! Named pipes library

use miow::pipe::{NamedPipe, NamedPipeBuilder};
use std;
use std::io;
use std::io::{Read, Write};
use std::sync::atomic::*;
use std::sync::{Arc, Mutex};
use jsonrpc_core::{Metadata, MetaIoHandler, Middleware, NoopMiddleware};
use jsonrpc_server_utils::reactor;
use jsonrpc_server_utils::tokio_core::reactor::Remote;
use validator;

pub type Result<T> = std::result::Result<T, Error>;

const MAX_REQUEST_LEN: u32 = 65536;
const REQUEST_READ_BATCH: usize = 4096;

#[derive(Debug)]
pub enum Error {
	Io(std::io::Error),
	NotStarted,
	AlreadyStopping,
	NotStopped,
	IsStopping,
}

impl std::convert::From<std::io::Error> for Error {
	fn from(io_error: std::io::Error) -> Error {
		Error::Io(io_error)
	}
}

pub struct PipeHandler<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	waiting_pipe: NamedPipe,
	handler: Arc<MetaIoHandler<M, S>>,
	// TODO [ToDr] To be used by async implementation of IPC.
	_remote: Remote,
}

impl<M: Metadata, S: Middleware<M>> PipeHandler<M, S> {
	/// start ipc rpc server
	pub fn start(
		addr: &str,
		handler: Arc<MetaIoHandler<M, S>>,
		remote: Remote,
	) -> Result<Self> {
		Ok(PipeHandler {
			waiting_pipe: try!(
				NamedPipeBuilder::new(addr)
					.first(true)
					.accept_remote(true)
					.max_instances(255)
					.inbound(true)
					.outbound(true)
					.out_buffer_size(MAX_REQUEST_LEN)
					.in_buffer_size(MAX_REQUEST_LEN)
					.create()
			),
			handler: handler,
			_remote: remote,
		})
	}

	fn handle_incoming(&mut self, addr: &str, stop: Arc<AtomicBool>) -> io::Result<()> {
		trace!(target: "ipc", "Waiting for client: [{}]", addr);

		// blocking wait with small timeouts
		// allows check if the server is actually stopped to quit gracefully
		// (`connect` does not allow that, it will block indefinitely)
		loop {
			if let Ok(_) = NamedPipe::wait(addr, Some(200)) {
				try!(self.waiting_pipe.connect());
				trace!(target: "ipc", "Received connection to address [{}]", addr);
				break;
			}
			if stop.load(Ordering::Relaxed) {
				trace!(target: "ipc", "Stopped listening sequence [{}]", addr);
				return Ok(())
			}
		}

		let mut connected_pipe = std::mem::replace::<NamedPipe>(&mut self.waiting_pipe,
			try!(NamedPipeBuilder::new(addr)
				.first(false)
				.accept_remote(true)
				.inbound(true)
				.outbound(true)
				.out_buffer_size(MAX_REQUEST_LEN)
				.in_buffer_size(MAX_REQUEST_LEN)
				.create()));

		let thread_handler = self.handler.clone();
		std::thread::spawn(move || {
			let mut buf = vec![0u8; MAX_REQUEST_LEN as usize];
			let mut fin = REQUEST_READ_BATCH;
			loop {
				let start = fin - REQUEST_READ_BATCH;
				trace!(target: "ipc", "Reading {} - {} of the buffer", start, fin);
				match connected_pipe.read(&mut buf[start..fin]) {
					Ok(size) => {
						let (requests, last_index) = {
							let effective = &buf[0..start + size];
							fin = fin + size;
							trace!(target: "ipc", "Received rpc data: {} bytes", effective.len());

							validator::extract_requests(effective)
						};
						if requests.len() > 0 {
							let mut response_buf = Vec::new();
							for rpc_msg in requests  {
								trace!(target: "ipc", "Request: {}", rpc_msg);
								let meta = Default::default();
								let response: Option<String> = thread_handler.handle_request_sync(&rpc_msg, meta);

								if let Some(response_str) = response {
									trace!(target: "ipc", "Response: {}", &response_str);
									response_buf.extend(response_str.into_bytes());
								}
							}

							if let Err(write_err) = connected_pipe.write_all(&response_buf[..]).and_then(|_| connected_pipe.flush()) {
								trace!(target: "ipc", "Response write error: {:?}", write_err);
							}
							else {
								trace!(target: "ipc", "Sent rpc response: {} bytes", response_buf.len());
							}

							let leftover_len = start + size - (last_index + 1);
							if leftover_len > 0 {
								let leftover = buf[last_index + 1..start + size].to_vec();
								buf[0..leftover_len].copy_from_slice(&leftover[..]);
							}
							fin = leftover_len + REQUEST_READ_BATCH;
						}
						else { continue; }
					},
					Err(e) => {
						// closed connection
						trace!(target: "ipc", "Dropped connection {:?}", e);
						break;
					}
				}
			}
		});

		Ok(())
	}
}

pub struct Server<M: Metadata = (), S: Middleware<M> = NoopMiddleware> {
	is_stopping: Arc<AtomicBool>,
	is_stopped: Arc<AtomicBool>,
	io_handler: Arc<MetaIoHandler<M, S>>,
	remote_handle: Mutex<Option<reactor::Remote>>,
	remote: Remote,
	addr: String,
}

impl<M: Metadata, S: Middleware<M>> Server<M, S> {
	/// New server
	pub fn new<T>(socket_addr: &str, io_handler: T) -> Result<Self> where
		T: Into<MetaIoHandler<M, S>>,
	{
		Self::with_remote(
			socket_addr,
			io_handler,
			reactor::UninitializedRemote::Unspawned,
		)
	}

	/// New Server using RpcHandler
	pub fn with_remote<T:>(
		socket_addr: &str,
		io_handler: T,
		remote: reactor::UninitializedRemote,
	) -> Result<Server<M, S>> where
		T: Into<MetaIoHandler<M, S>>,
	{
		let remote = remote.initialize()?;

		Ok(Server {
			is_stopping: Arc::new(AtomicBool::new(false)),
			is_stopped: Arc::new(AtomicBool::new(true)),
			io_handler: Arc::new(io_handler.into()),
			remote: remote.remote(),
			remote_handle: Mutex::new(Some(remote)),
			addr: socket_addr.to_owned(),
		})
	}

	/// Run server (in this thread)
	pub fn run(&self) -> Result<()> {
		let mut pipe_handler = PipeHandler::start(
			&self.addr,
			self.io_handler.clone(),
			self.remote.clone(),
		)?;
		loop  {
			try!(pipe_handler.handle_incoming(&self.addr, Arc::new(AtomicBool::new(false))));
		}
	}

	/// Run server (in separate thread)
	pub fn run_async(&self) -> Result<()> {
		if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::IsStopping) }
		if !self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStopped) }

		trace!(target: "ipc", "Started named pipes server [{}]", self.addr);

		let thread_stopping = self.is_stopping.clone();
		let thread_stopped = self.is_stopped.clone();
		let thread_handler = self.io_handler.clone();
		let addr = self.addr.clone();
		let remote = self.remote.clone();
		std::thread::spawn(move || {
			let mut pipe_handler = PipeHandler::start(
				&addr,
				thread_handler,
				remote,
			).unwrap();
			while !thread_stopping.load(Ordering::Relaxed) {
				trace!(target: "ipc", "Accepting pipe connection");
				if let Err(pipe_listener_error) = pipe_handler.handle_incoming(&addr, thread_stopping.clone()) {
					trace!(target: "ipc", "Pipe listening error: {:?}", pipe_listener_error);
				}
			}
			thread_stopped.store(true, Ordering::Relaxed);
		});

		self.is_stopped.store(false, Ordering::Relaxed);
		Ok(())
	}

	pub fn stop_async(&self) -> Result<()> {
		self.remote_handle.lock().unwrap().take().map(|s| s.close());
		if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
		if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
		self.is_stopping.store(true, Ordering::Relaxed);
		Ok(())
	}

	pub fn stop(&self) -> Result<()> {
		self.remote_handle.lock().unwrap().take().map(|s| s.close());
		if self.is_stopped.load(Ordering::Relaxed) { return Err(Error::NotStarted) }
		if self.is_stopping.load(Ordering::Relaxed) { return Err(Error::AlreadyStopping)}
		self.is_stopping.store(true, Ordering::Relaxed);
		while !self.is_stopped.load(Ordering::Relaxed) { std::thread::park_timeout(std::time::Duration::new(0, 50)); }
		Ok(())
	}
}

impl<M: Metadata, S: Middleware<M>> Drop for Server<M, S> {
	fn drop(&mut self) {
		self.stop_async().unwrap_or_else(|_| {}); // ignore error - can be stopped already
		// todo : no stable logging for windows?
		trace!(target: "ipc", "IPC Server : shutdown");
	}
}

pub fn server<I, M: Metadata, S: Middleware<M>>(
	io: I, 
	path: &str
) -> Result<Server<M, S>>
	where I: Into<MetaIoHandler<M, S>>
{
	let server = Server::new(path, io)?;
	server.run_async()?;

	Ok(server)
}