// because we reuse the type names as idents in the macros as a dirty hack to
// work around `concat_idents!` being unstable.
#![allow(non_snake_case)]

///! Automatically serialize and deserialize parameters around a strongly-typed function.

use jsonrpc_core::{self, Error, Params, Value, Metadata};
use jsonrpc_core::futures::{self, BoxFuture, Future};
use serde::{Serialize, Deserialize};
use util::{invalid_params, expect_no_params};

fn to_value<T>(value: T) -> Value where T: Serialize {
	jsonrpc_core::to_value(value).expect("Expected always-serializable type.")
}

/// Auto-generates an RPC trait from trait definition.
///
/// This just copies out all the methods, docs, and adds another
/// function `to_delegate` which will automatically wrap each strongly-typed
/// function in a wrapper which handles parameter and output type serialization.
///
/// RPC functions may come in a couple forms: async and synchronous.
/// These are parsed with the custom `#[rpc]` attribute, which must follow
/// documentation.
///
/// ## The #[rpc] attribute
///
/// Valid forms:
///  - `#[rpc(name = "name_here")]` (a synchronous rpc function which should be bound to the given name)
///  - `#[rpc(async, name = "name_here")]` (an async rpc function which should be bound to the given name)
///
/// Synchronous function format:
/// `fn foo(&self, Param1, Param2, Param3) -> Result<Out, Error>`.
///
/// Asynchronous RPC functions must come in this form:
/// `fn foo(&self, Param1, Param2, Param3) -> BoxFuture<Out, Error>;
///
/// Anything else will be rejected by the code generator.
#[macro_export]
macro_rules! build_rpc_trait {
	// entry-point. todo: make another for traits w/ bounds.
	(
		$(#[$t_attr: meta])*
		pub trait $name: ident {
			$(
				$( #[doc=$m_doc:expr] )*
				#[ rpc( $($t:tt)* ) ]
				fn $m_name: ident ( $($p: tt)* ) -> $result: tt <$out: ty, Error>;
			)*
		}
	) => {
		$(#[$t_attr])*
		pub trait $name: Sized + Send + Sync + 'static {
			$(
				$(#[doc=$m_doc])*
				fn $m_name ( $($p)* ) -> $result<$out, Error> ;
			)*

			/// Transform this into an `IoDelegate`, automatically wrapping
			/// the parameters.
			fn to_delegate<M: ::jsonrpc_core::Metadata>(self) -> $crate::IoDelegate<Self, M> {
				let mut del = $crate::IoDelegate::new(self.into());
				$(
					build_rpc_trait!(WRAP del =>
						( $($t)* )
						fn $m_name ( $($p)* ) -> $result <$out, Error>
					);
				)*
				del
			}
		}
	};

	// entry-point for trait with metadata methods
	(
		$(#[$t_attr: meta])*
		pub trait $name: ident {
			type Metadata;
			$(
				$( #[doc=$m_doc:expr] )*
				#[ rpc( $($t:tt)* ) ]
				fn $m_name: ident ( $($p: tt)* ) -> $result: tt <$out: ty, Error>;
			)*
		}
	) => {
		$(#[$t_attr])*
		pub trait $name: Sized + Send + Sync + 'static {
			/// Requests metadata
			type Metadata: ::jsonrpc_core::Metadata;

			$(
				$(#[doc=$m_doc])*
				fn $m_name ( $($p)* ) -> $result<$out, Error> ;
			)*

			/// Transform this into an `IoDelegate`, automatically wrapping
			/// the parameters.
			fn to_delegate(self) -> $crate::IoDelegate<Self, Self::Metadata> {
				let mut del = $crate::IoDelegate::new(self.into());
				$(
					build_rpc_trait!(WRAP del =>
						( $($t)* )
						fn $m_name ( $($p)* ) -> $result <$out, Error>
					);
				)*
				del
			}
		}
	};

	( WRAP $del: expr =>
		(name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self $(, $param: ty)*) -> $result: tt <$out: ty, Error>
	) => {
		$del.add_method($name, move |base, params| {
			$crate::Wrap::wrap_rpc(&(Self::$method as fn(&_ $(, $param)*) -> $result <$out, Error>), base, params)
		});
		$(
			$(
				$del.add_alias($name, $alias);
			)+
		)*
	};

	( WRAP $del: expr =>
		(async, name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self $(, $param: ty)*) -> $result: tt <$out: ty, Error>
	) => {
		$del.add_async_method($name, move |base, params| {
			$crate::WrapAsync::wrap_rpc(&(Self::$method as fn(&_ $(, $param)*) -> $result <$out, Error>), base, params)
		});
		$(
			$(
				$del.add_alias($name, $alias);
			)+
		)*
	};

	( WRAP $del: expr =>
		(meta, name = $name: expr $(, alias = [ $( $alias: expr, )+ ])*)
		fn $method: ident (&self, Self::Metadata $(, $param: ty)*) -> $result: tt <$out: ty, Error>
	) => {
		$del.add_method_with_meta($name, move |base, params, meta| {
			$crate::WrapMeta::wrap_rpc(&(Self::$method as fn(&_, Self::Metadata $(, $param)*) -> $result <$out, Error>), base, params, meta)
		});
		$(
			$(
				$del.add_alias($name, $alias);
			)+
		)*
	};
}

/// A wrapper type without an implementation of `Deserialize`
/// which allows a special implementation of `Wrap` for functions
/// that take a trailing default parameter.
pub struct Trailing<T: Default + Deserialize>(pub T);

/// Wrapper trait for synchronous RPC functions.
pub trait Wrap<B: Send + Sync + 'static> {
	fn wrap_rpc(&self, base: &B, params: Params) -> Result<Value, Error>;
}

/// Wrapper trait for asynchronous RPC functions.
pub trait WrapAsync<B: Send + Sync + 'static> {
	fn wrap_rpc(&self, base: &B, params: Params) -> BoxFuture<Value, Error>;
}

/// Wrapper trait for meta RPC functions.
pub trait WrapMeta<B: Send + Sync + 'static, M: Metadata> {
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> BoxFuture<Value, Error>;
}

// special impl for no parameters.
impl<B, OUT> Wrap<B> for fn(&B) -> Result<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static
{
	fn wrap_rpc(&self, base: &B, params: Params) -> Result<Value, Error> {
		expect_no_params(params)
			.and_then(|()| (self)(base))
			.map(to_value)
	}
}

impl<B, OUT> WrapAsync<B> for fn(&B) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static
{
	fn wrap_rpc(&self, base: &B, params: Params) -> BoxFuture<Value, Error> {
		match expect_no_params(params) {
			Ok(()) => (self)(base).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}

impl<B, M, OUT> WrapMeta<B, M> for fn(&B, M) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, M: Metadata
{
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> BoxFuture<Value, Error> {
		match expect_no_params(params) {
			Ok(()) => (self)(base, meta).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}


// creates a wrapper implementation which deserializes the parameters,
// calls the function with concrete type, and serializes the output.
macro_rules! wrap {
	($($x: ident),+) => {

		// synchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
		> Wrap<BASE> for fn(&BASE, $($x,)+) -> Result<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> Result<Value, Error> {
				params.parse::<($($x,)+)>().and_then(|($($x,)+)| {
					(self)(base, $($x,)+)
				}).map(to_value)
			}
		}

		// asynchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
		> WrapAsync<BASE> for fn(&BASE, $($x,)+ ) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> BoxFuture<Value, Error> {
				match params.parse::<($($x,)+)>() {
					Ok(($($x,)+)) => (self)(base, $($x,)+).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}

		// asynchronous implementation with meta
		impl <
			BASE: Send + Sync + 'static,
			META: Metadata,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
		> WrapMeta<BASE, META> for fn(&BASE, META, $($x,)+) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META) -> BoxFuture<Value, Error> {
				match params.parse::<($($x,)+)>() {
					Ok(($($x,)+)) => (self)(base, meta, $($x,)+).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}
	}
}

fn params_len(params: &Params) -> Result<usize, Error> {
	match *params {
		Params::Array(ref v) => Ok(v.len()),
		Params::None => Ok(0),
		_ => Err(invalid_params("not an array", "")),
	}
}

fn parse_trailing_param<T: Default + Deserialize>(params: Params) -> Result<(T, ), Error> {
	let len = try!(params_len(&params));
	let id = match len {
		0 => Ok((T::default(),)),
		1 => params.parse::<(T,)>(),
		_ => Err(Error::invalid_params("Expected 0 or 1 parameters.")),
	};

	id
}

// special impl for no parameters other than block parameter.
impl<B, OUT, T> Wrap<B> for fn(&B, Trailing<T>) -> Result<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, T: Default + Deserialize
{
	fn wrap_rpc(&self, base: &B, params: Params) -> Result<Value, Error> {
		let id = try!(parse_trailing_param(params)).0;

		(self)(base, Trailing(id)).map(to_value)
	}
}

impl<B, OUT, T> WrapAsync<B> for fn(&B, Trailing<T>) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, T: Default + Deserialize
{
	fn wrap_rpc(&self, base: &B, params: Params) -> BoxFuture<Value, Error> {
		let id = parse_trailing_param(params);

		match id {
			Ok((id,)) => (self)(base, Trailing(id)).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}

impl<B, M, OUT, T> WrapMeta<B, M> for fn(&B, M, Trailing<T>) -> BoxFuture<OUT, Error>
	where B: Send + Sync + 'static, OUT: Serialize + 'static, T: Default + Deserialize, M: Metadata,
{
	fn wrap_rpc(&self, base: &B, params: Params, meta: M) -> BoxFuture<Value, Error> {
		let id = parse_trailing_param(params);

		match id {
			Ok((id,)) => (self)(base, meta, Trailing(id)).map(to_value).boxed(),
			Err(e) => futures::failed(e).boxed(),
		}
	}
}

// similar to `wrap!`, but handles a single default trailing parameter
// accepts an additional argument indicating the number of non-trailing parameters.
macro_rules! wrap_with_trailing {
	($num: expr, $($x: ident),+) => {
		// synchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
			TRAILING: Default + Deserialize,
		> Wrap<BASE> for fn(&BASE, $($x,)+ Trailing<TRAILING>) -> Result<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> Result<Value, Error> {
				let len = try!(params_len(&params));

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ TRAILING::default())),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ id)),
					_ => Err(Error::invalid_params("Expected 0 or 1 parameters.")),
				};

				let ($($x,)+ id) = try!(params);
				(self)(base, $($x,)+ Trailing(id)).map(to_value)
			}
		}

		// asynchronous implementation
		impl <
			BASE: Send + Sync + 'static,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
			TRAILING: Default + Deserialize,
		> WrapAsync<BASE> for fn(&BASE, $($x,)+ Trailing<TRAILING>) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params) -> BoxFuture<Value, Error> {
				let len = match params_len(&params) {
					Ok(len) => len,
					Err(e) => return futures::failed(e).boxed(),
				};

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ TRAILING::default())),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ id)),
					_ => Err(Error::invalid_params("Expected 0 or 1 parameters.")),
				};

				match params {
					Ok(($($x,)+ id)) => (self)(base, $($x,)+ Trailing(id)).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}

		// asynchronous implementation with meta
		impl <
			BASE: Send + Sync + 'static,
			META: Metadata,
			OUT: Serialize + 'static,
			$($x: Deserialize,)+
			TRAILING: Default + Deserialize,
		> WrapMeta<BASE, META> for fn(&BASE, META, $($x,)+ Trailing<TRAILING>) -> BoxFuture<OUT, Error> {
			fn wrap_rpc(&self, base: &BASE, params: Params, meta: META) -> BoxFuture<Value, Error> {
				let len = match params_len(&params) {
					Ok(len) => len,
					Err(e) => return futures::failed(e).boxed(),
				};

				let params = match len - $num {
					0 => params.parse::<($($x,)+)>()
						.map(|($($x,)+)| ($($x,)+ TRAILING::default())),
					1 => params.parse::<($($x,)+ TRAILING)>()
						.map(|($($x,)+ id)| ($($x,)+ id)),
					_ => Err(Error::invalid_params("Expected 0 or 1 parameters.")),
				};

				match params {
					Ok(($($x,)+ id)) => (self)(base, meta, $($x,)+ Trailing(id)).map(to_value).boxed(),
					Err(e) => futures::failed(e).boxed(),
				}
			}
		}
	}
}

wrap!(A, B, C, D, E);
wrap!(A, B, C, D);
wrap!(A, B, C);
wrap!(A, B);
wrap!(A);

wrap_with_trailing!(5, A, B, C, D, E);
wrap_with_trailing!(4, A, B, C, D);
wrap_with_trailing!(3, A, B, C);
wrap_with_trailing!(2, A, B);
wrap_with_trailing!(1, A);

