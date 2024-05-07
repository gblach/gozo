# Gozo

Message scheduler for [NATS](https://nats.io).

Gozo connects to the NATS server (with JetStream enabled) and listens for a message
with a subject `gozo`, a reply subject, payload and relevant headers.
When one is received, it schedules to send back a message with the reply subject
as the message subject, the same payload and the headers described in the usage section.
Due to all communication is via NATS, Gozo can be used in any programming language
that has [client libraries](https://nats.io/download/#nats-clients).

## Options

Options can be set in two ways: from the command line or from an environment variable.
Command line options are listed below. All environment variables are written in upper case
with a `NATS_` prefix, e.g. `--address` option becomes `NATS_ADDRESS`.
On startup gozo reads environment variables from `/etc/gozo.env` and `./gozo.env` files if it exits.
If both the command line option and the environment variable are set,
the command line option is used.

+ __-a, --address:__ set address to NATS server, default: nats://localhost:4222
+ __-s, --secure:__ enforce secure (TLS) connection
+ __-t, --token:__ set token authentication
+ __-u, --user:__ set username
+ __-p, --password:__ set password
+ __-c, --cert:__ set client TLS certificate
+ __-k, --key:__ set client TLS key
+ __-n, --nkey:__ set NKey authentication
+ __-j, --jwt:__ set path to credentials file

## Usage

For readability, all examples below are written in Python.

### Schedule a message with absolute time

`Gozo-When` header contains a string with the Unix epoch timestamp
when the message will be sent back.

`Gozo-Id` header is optional, but if present it must be unique.
Schedules with `Gozo-Id` header can be overwritten or canceled.
In addition, schedules with `Gozo-Id` are written to the NATS key/value store,
so they won't be discarded when gozo is restarted.

```python
import asyncio
import nats
import time
import uuid

async def message_cb(msg):
	print(msg.subject) # 'reply.subject'
	print(msg.data) # b'Hello World!'
	print(msg.headers) # {'Gozo-Reply': 'Yes', 'Gozo-Id': 'c8d816dd-578b-47ff-84c1-031f3ee7ade3'}

async def main():
	nc = await nats.connect('nats://localhost:4222')
	sub = await nc.subscribe('reply.subject', cb=message_cb)
	headers = {
		'Gozo-When': str(int(time.time()) + 10),
		'Gozo-Id': str(uuid.uuid4()),
	}
	await nc.publish('gozo', b'Hello Gozo!', 'reply.subject', headers)
	await asyncio.sleep(11)

if __name__ == '__main__':
	asyncio.run(main())
```

### Schedule a message with relative time

This time `Gozo-When` header contains a string with a "+" sign followed by the number of seconds
after which the message will be sent back.

```python
import asyncio
import nats
import uuid

async def message_cb(msg):
	print(msg.subject) # 'reply.subject'
	print(msg.data) # b'Hello World!'
	print(msg.headers) # {'Gozo-Reply': 'Yes', 'Gozo-Id': 'c8d816dd-578b-47ff-84c1-031f3ee7ade3'}

async def main():
	nc = await nats.connect('nats://localhost:4222')
	sub = await nc.subscribe('reply.subject', cb=message_cb)
	headers = {
		'Gozo-When': '+10',
		'Gozo-Id': str(uuid.uuid4()),
	}
	await nc.publish('gozo', b'Hello Gozo!', 'reply.subject', headers)
	await asyncio.sleep(11)

if __name__ == '__main__':
	asyncio.run(main())
```

### Cancel a schedule

To cancel the schedule, send a message with the header `Gozo-Del-Id`
and set the corresponding ID as its value.

```python
import asyncio
import nats

async def main():
	nc = await nats.connect('nats://localhost:4222')
	headers = {
		'Gozo-Del-Id': 'c8d816dd-578b-47ff-84c1-031f3ee7ade3',
	}
	await nc.publish('gozo', b'', None, headers)

if __name__ == '__main__':
	asyncio.run(main())
```
