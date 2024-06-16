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

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
	when: u64,
	subject: String,
	payload: bytes::Bytes,
}

#[derive(Clone)]
pub struct Sched {
	pub entry: Vec<Entry>,
	pub id: Vec<Option<String>>,
	pub id_loc: BTreeMap<String, usize>,
}

impl Sched {
	pub fn new() -> Sched {
		Sched {
			entry: Vec::new(),
			id: Vec::new(),
			id_loc: BTreeMap::new(),
		}
	}
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

		if let Some(entry) = sched.entry.first() {
			if entry.when <= now() {
				let mut headers = HeaderMap::new();
				headers.insert("Gozo-Reply", "Yes");
				if let Some(id) = &sched.id.first().unwrap() {
					let id = HeaderValue::from_str(id).unwrap();
					headers.insert("Gozo-Id", id);
				}

				let _ = nats.publish_with_headers(
					entry.subject.clone(), headers,
					entry.payload.clone()).await;

				sched.entry.remove(0);
				if let Some(id) = sched.id.remove(0) {
					sched.id_loc.remove(&id);
					let kv = kv.clone();
					tokio::spawn(async move {
						let _ = kv.delete(id).await;
					});
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
		sched.entry.remove(idx);
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

				let idx = sched.entry.partition_point(|x| x.when <= when);

				let entry = Entry {
					when,
					subject: reply.to_string(),
					payload: msg.payload,
				};

				sched.entry.insert(idx, entry.clone());
				sched.id.insert(idx, id.clone());

				if let Some(id) = id {
					sched.id_loc.insert(id.clone(), idx);
					let entry = rmp_serde::to_vec_named(&entry).unwrap();
					let _ = kv.put(id, entry.into()).await;
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
		let entry = kv.get(id.clone()).await?.unwrap();
		let entry: Entry = rmp_serde::from_slice(&entry).unwrap();
		let idx = sched.entry.partition_point(|x| x.when <= entry.when);

		sched.entry.insert(idx, entry);
		sched.id.insert(idx, Some(id.clone()));
		sched.id_loc.insert(id, idx);
	}

	Ok(())
}
