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

use std;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio_core::reactor::Core;
use tokio_core::net::TcpListener;
use tokio_core::io::Io;
use futures::{future, Future, Stream, Sink};
use tokio_service::Service as TokioService;

use jsonrpc::{Metadata, MetaIoHandler};
use service::{Service, SocketMetadata};
use line_codec::LineCodec;

struct Server {
    listen_addr: SocketAddr,
    handler: Arc<MetaIoHandler<SocketMetadata>>,
}

impl Server {
    fn new(addr: SocketAddr, handler: Arc<MetaIoHandler<SocketMetadata>>) -> Self {
        Server { listen_addr: addr, handler: handler }
    }

    fn spawn_service(&self, peer_addr: SocketAddr) -> Service {
        Service::new(peer_addr, self.handler.clone())
    }

    fn run(&self) -> std::io::Result<()> {
        let mut core = Core::new()?;
        let handle = core.handle();

        let listener = TcpListener::bind(&self.listen_addr, &handle)?;

        let connections = listener.incoming();
        let server = connections.for_each(move |(socket, peer_addr)| {
            let (writer, reader) = socket.framed(LineCodec).split();
            let mut service = self.spawn_service(peer_addr);

            let responses = reader.and_then(
                move |req| service.call(req).then(|response|
                    match response {
                        Err(e) => {
                            warn!(target: "tcp", "Error while processing request: {:?}", e);
                            future::ok(String::new())
                        },
                        Ok(None) => {
                            trace!(target: "tcp", "JSON RPC request produced no response");
                            future::ok(String::new())
                        },
                        Ok(Some(response_data)) => future::ok(response_data),
                    }));

            let server = writer.send_all(responses).then(|_| Ok(()));
            handle.spawn(server);

            Ok(())
        });

        core.run(server)
    }
}
