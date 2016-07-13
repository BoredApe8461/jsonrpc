extern crate jsonrpc_core;

use std::str::Lines;
use std::net::TcpStream;
use std::io::{Read, Write};
use std::sync::Arc;
use self::jsonrpc_core::IoHandler;
use super::*;

fn serve() -> Server {
		let rpc = Arc::new(IoHandler::new());
		let cors_domains = vec![AccessControlAllowOrigin::Value("ethcore.io".into())];

		Server::start(&"127.0.0.1:0".parse().unwrap(), rpc, cors_domains).unwrap()
}

struct Response {
	status: String,
	headers: String,
	body: String,
}

fn read_block(lines: &mut Lines) -> String {
	let mut block = String::new();
	loop {
		let line = lines.next();
		match line {
			Some("") | None => break,
			Some(v) => {
				block.push_str(v);
				block.push_str("\n");
			},
		}
	}
	block
}

fn request(server: Server, request: &str) -> Response {
	let mut req = TcpStream::connect(server.addr()).unwrap();
	req.write_all(request.as_bytes()).unwrap();

	let mut response = String::new();
	req.read_to_string(&mut response).unwrap();

	let mut lines = response.lines();
	let status = lines.next().unwrap().to_owned();
	let headers =	read_block(&mut lines);
	let body = read_block(&mut lines);

	Response {
		status: status,
		headers: headers,
		body: body,
	}
}

#[test]
fn should_return_method_not_allowed_for_get() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			GET / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			\r\n\
			I shouldn't be read.\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 405 Method Not Allowed".to_owned());
	assert_eq!(response.body, "3D\nUsed HTTP Method is not allowed. POST or OPTIONS is required\n".to_owned());
}

#[test]
fn should_return_unsupported_media_type_if_not_json() {
	// given
	let server = serve();

	// when
	let response = request(server,
		"\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			\r\n\
			{}\r\n\
		"
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 415 Unsupported Media Type".to_owned());
	assert_eq!(response.body, "51\nSupplied content type is not allowed. Content-Type: application/json is required\n".to_owned());
}

#[test]
fn should_return_error_for_malformed_request() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"3.0","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_error_for_malformed_request2() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","metho1d":""}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, invalid_request());
}

#[test]
fn should_return_empty_response_for_notification() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, "0\n".to_owned());
}


#[test]
fn should_return_method_not_found() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
}

#[test]
fn should_add_cors_headers() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Origin: ethcore.io\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(response.headers.contains("Access-Control-Allow-Origin: ethcore.io"), "Headers missing in {}", response.headers);
}

#[test]
fn should_not_add_cors_headers() {
	// given
	let server = serve();

	// when
	let req = r#"{"jsonrpc":"2.0","id":"1","method":"x"}"#;
	let response = request(server,
		&format!("\
			POST / HTTP/1.1\r\n\
			Host: 127.0.0.1:8080\r\n\
			Origin: fake.io\r\n\
			Connection: close\r\n\
			Content-Type: application/json\r\n\
			Content-Length: {}\r\n\
			\r\n\
			{}\r\n\
		", req.as_bytes().len(), req)
	);

	// then
	assert_eq!(response.status, "HTTP/1.1 200 OK".to_owned());
	assert_eq!(response.body, method_not_found());
	assert!(response.headers.contains("Access-Control-Allow-Origin: null"), "Headers missing in {}", response.headers);
}


fn method_not_found() -> String {
 "59\n{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32601,\"message\":\"Method not found\",\"data\":null},\"id\":1}\n0\n".to_owned()
}

fn invalid_request() -> String {
 "5B\n{\"jsonrpc\":\"2.0\",\"error\":{\"code\":-32600,\"message\":\"Invalid request\",\"data\":null},\"id\":null}\n0\n".to_owned()
}
