//  This Source Code Form is subject to the terms of the Mozilla Public
//  License, v. 2.0. If a copy of the MPL was not distributed with this
//  file, You can obtain one at http://mozilla.org/MPL/2.0/.

mod gozo;
use argh::FromArgs;
use async_nats::jetstream::kv;
use futures::stream::StreamExt;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(FromArgs)]
/// Message scheduler for NATS
struct Args {
	/// set address to NATS server, default: nats://localhost:4222
	#[argh(option, short='a')]
	address: Option<String>,

	/// enforce secure (TLS) connection
	#[argh(switch, short='s')]
	secure: bool,

	/// set token authentication
	#[argh(option, short='t')]
	token: Option<String>,

	/// set username
	#[argh(option, short='u')]
	user: Option<String>,

	/// set password
	#[argh(option, short='p')]
	password: Option<String>,

	/// set client TLS certificate
	#[argh(option, short='c')]
	cert: Option<String>,

	/// set client TLS key
	#[argh(option, short='k')]
	key: Option<String>,

	/// set NKey authentication
	#[argh(option, short='n')]
	nkey: Option<String>,

	/// set path to credentials file
	#[argh(option, short='j')]
	jwt: Option<String>,
}

impl Args {
	fn get(&self, field: &str, default: Option<String>) -> Option<String> {
		let mut value = match field {
			"address"   => self.address.clone(),
			"token"     => self.token.clone(),
			"user"      => self.user.clone(),
			"password"  => self.password.clone(),
			"cert"      => self.cert.clone(),
			"key"       => self.key.clone(),
			"nkey"      => self.nkey.clone(),
			"jwt"       => self.jwt.clone(),
			_           => None,
		};

		if value.is_none() {
			let envvar = format!("NATS_{}", field.to_uppercase());
			value = std::env::var(envvar).ok();
		}

		if value.is_none() && default.is_some() {
			value = default;
		}

		value
	}

	fn get_bool(&self, field: &str) -> bool {
		let mut value = match field {
			"secure"    => self.secure,
			_           => false,
		};

		if !value {
			let envvar = format!("NATS_{}", field.to_uppercase());
			value = !std::env::var(envvar).unwrap_or("".to_string()).is_empty();
		}

		value
	}
}

#[tokio::main]
async fn main() -> Result<(), async_nats::Error> {
	let args: Args = argh::from_env();
	dotenv::from_filename("./gozo.env").ok();
	dotenv::from_filename("/etc/gozo.env").ok();

	let address = args.get("address", Some("nats://localhost:4222".to_string())).unwrap();

	let mut options = async_nats::ConnectOptions::new()
		.require_tls(args.get_bool("secure"));

	if let Some(token) = args.get("token", None) {
		options = options.token(token);
	}

	if let (Some(user), Some(password)) = (args.get("user", None), args.get("password", None)) {
		options = options.user_and_password(user, password);
	}

	if let (Some(cert), Some(key)) = (args.get("cert", None), args.get("key", None)) {
    		options = options.add_client_certificate(cert.into(), key.into());
    	}

	if let Some(nkey) = args.get("nkey", None) {
		options = options.nkey(nkey);
	}

	if let Some(jwt) = args.get("jwt", None) {
		options = options.credentials_file(jwt).await?;
	}

	ctrlc::set_handler(|| std::process::exit(0)).ok();

	let nats = async_nats::connect_with_options(address, options).await?;
	let jetstream = async_nats::jetstream::new(nats.clone());
	let kv = jetstream.create_key_value(kv::Config {
		bucket: "gozo".to_string(),
		..Default::default()
	}).await?;

	let sched: gozo::SchedMutex = Arc::new(Mutex::new(gozo::Sched::new()));
	gozo::schedule_load(kv.clone(), sched.clone()).await?;
	tokio::spawn(gozo::replyloop(nats.clone(), kv.clone(), sched.clone()));

	let mut sub = nats.subscribe("gozo").await?;
	while let Some(msg) = sub.next().await {
		gozo::schedule(kv.clone(), sched.clone(), msg).await;
	}

	Ok(())
}
