//  This Source Code Form is subject to the terms of the Mozilla Public
//  License, v. 2.0. If a copy of the MPL was not distributed with this
//  file, You can obtain one at http://mozilla.org/MPL/2.0/.

use async_nats::{ jetstream::kv, Client, HeaderMap, HeaderValue, Message };
use futures::stream::TryStreamExt;
use serde::{ Serialize, Deserialize };
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{ Duration, SystemTime };
use tokio::sync::{ Mutex, MutexGuard };

#[derive(Clone)]
pub struct Sched {
	pub when: Vec<u64>,
	pub subject: Vec<String>,
	pub payload: Vec<bytes::Bytes>,
	pub id: Vec<Option<String>>,
	pub id_loc: BTreeMap<String, usize>,
}

impl Sched {
	pub fn new() -> Sched {
		Sched {
			when: Vec::new(),
			subject: Vec::new(),
			payload: Vec::new(),
			id: Vec::new(),
			id_loc: BTreeMap::new(),
		}
	}
}

#[derive(Serialize, Deserialize, Debug)]
pub struct KvSched {
	when: u64,
	subject: String,
	payload: bytes::Bytes,
}

pub type SchedMutex = Arc<Mutex<Sched>>;
pub type SchedGuard<'a> = MutexGuard<'a, Sched>;

fn now() -> u64 {
	SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
}

pub async fn replyloop(nats: Client, kv: kv::Store, sched_mutex: SchedMutex) {
	let mut interval = tokio::time::interval(Duration::new(1, 0));

	loop {
		interval.tick().await;
		let mut sched = sched_mutex.lock().await;

		if let Some(when) = sched.when.first() {
			if *when <= now() {
				let mut headers = HeaderMap::new();
				headers.insert("Gozo-Reply", "Yes");
				if let Some(id) = &sched.id.first().unwrap() {
					let id = HeaderValue::from_str(id).unwrap();
					headers.insert("Gozo-Id", id);
				}

				let _ = nats.publish_with_headers(
					sched.subject.first().unwrap().clone(), headers,
					sched.payload.first().unwrap().clone()).await;

				sched.when.remove(0);
				sched.subject.remove(0);
				sched.payload.remove(0);
				if let Some(id) = sched.id.remove(0) {
					sched.id_loc.remove(&id);
					let _ = kv.delete(id).await;
				}

				interval.reset_immediately();
			}
		}
	}
}

fn get_when(when: &str) -> Result<u64, std::num::ParseIntError> {
	if when.starts_with('+') {
		let when = when.trim_start_matches('+');
		Ok(now() + when.parse::<u64>()?)
	} else {
		Ok(when.parse::<u64>()?)
	}
}

fn schedule_delete(sched: &mut SchedGuard<'_>, id: String) {
	if let Some(idx) = sched.id_loc.remove(&id) {
		sched.when.remove(idx);
		sched.subject.remove(idx);
		sched.payload.remove(idx);
		sched.id.remove(idx);
	}
}

pub async fn schedule(kv: kv::Store, sched_mutex: SchedMutex, msg: Message) {
	if let Some(headers) = msg.headers {
		if let Some(when) = headers.get("Gozo-When") {
			if let (Ok(when), Some(reply)) = (get_when(when.as_str()), msg.reply) {
				let mut sched = sched_mutex.lock().await;

				let id = if let Some(id) = headers.get("Gozo-Id") {
					schedule_delete(&mut sched, id.to_string());
					Some(id.to_string())
				} else {
					None
				};

				let idx = sched.when.partition_point(|&x| x <= when);

				sched.when.insert(idx, when);
				sched.subject.insert(idx, reply.to_string());
				sched.payload.insert(idx, msg.payload.clone());
				sched.id.insert(idx, id.clone());

				if let Some(id) = id {
					sched.id_loc.insert(id.clone(), idx);

					let value = KvSched {
						when,
						subject: reply.to_string(),
						payload: msg.payload,
					};
					let value =
						rmp_serde::to_vec_named(&value).unwrap();
					let _ = kv.put(id, value.into()).await;
				}
			}
		} else if let Some(id) = headers.get("Gozo-Del-Id") {
			let mut sched = sched_mutex.lock().await;
			schedule_delete(&mut sched, id.to_string());
			let _ = kv.delete(id).await;
		}
	}
}

pub async fn schedule_load(kv: kv::Store, sched_mutex: SchedMutex)
	-> Result<(), async_nats::Error> {

	let mut ids = kv.keys().await?;
	let mut sched = sched_mutex.lock().await;

	while let Some(id) = ids.try_next().await? {
		let val = kv.get(id.clone()).await?.unwrap();
		let val: KvSched = rmp_serde::from_slice(&val).unwrap();
		let idx = sched.when.partition_point(|&x| x <= val.when);

		sched.when.insert(idx, val.when);
		sched.subject.insert(idx, val.subject.to_string());
		sched.payload.insert(idx, val.payload.clone());
		sched.id.insert(idx, Some(id.clone()));
		sched.id_loc.insert(id, idx);
	}

	Ok(())
}
