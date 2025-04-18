use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::builder::CreateMessage;
use serenity::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use std::time::Instant;
use serde::{Deserialize, Serialize};

const ENV_PATHS: &[&'static str] = &[
	"./env/",
	"../../env/",
];

const APP_DATA_FILE: &str = "appdata.bin";

const APP_WORD_LIST: &str = include_str!("../wordlist.txt");

#[derive(Debug, Default)]
struct TrieNode {
	children: HashMap<char, TrieNode>,
	end: bool,
}

#[derive(Debug, Default)]
struct Trie {
	root: TrieNode,
}

impl Trie {
	fn reset(&mut self) {
		self.root = TrieNode::default();
	}

	fn insert(&mut self, word: &str) {
		if word.len() == 0 {
			return;
		}
		let mut node= &mut self.root;
		for c in word.chars() {
			if !node.children.contains_key(&c) {
				node.children.insert(c, TrieNode::default());
			}
			node = node.children.get_mut(&c).unwrap();
		}
		node.end = true;
	}

	fn find_matches(&self, input: &str) -> Vec<(usize, usize)> {
		let mut matches = Vec::<(usize, usize)>::default();
		let mut cursor_it = input.chars();
		let mut local_it = cursor_it.clone();
		for start in 0..input.len() {
			// traverse the tree with the local iterator
			let mut node= &self.root;
			let mut end = 0;
			loop {
				match local_it.next() {
					Some(c) => {
						let v = node.children.get(&c);
						if v.is_some() {
							node = v.unwrap();
							if node.end {
								matches.push((start, start + end));
							}
							end += 1;
						}
						else {
							break
						}
					}
					None => {
						break
					}
				}
			}
			cursor_it.next();
			local_it = cursor_it.clone();
		}
		matches
	}

	fn find_word(&self, input: &str) -> bool {
		let mut node = &self.root;
		for c in input.chars() {
			if let Some(pair) = node.children.get_key_value(&c) {
				node = pair.1;
				if node.end {
					return true;
				}
				continue
			}
			return false;
		}
		false
	}
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct AppData {
	words: Vec<String>,
	admins: Vec<(String, u64)>,
}

impl AppData {
	fn build_trie(&self) -> Trie {
		let mut trie = Trie::default();
		for word in self.words.iter() {
			trie.insert(word.to_lowercase().as_str());
		}
		trie
	}
}

#[derive(Default)]
struct Handler {
	censor_list: RwLock<Trie>,
	allow_list: RwLock<Trie>,
	app_data: RwLock<AppData>,
}

#[async_trait]
impl EventHandler for Handler {
	// Set a handler for the `message` event. This is called whenever a new message is received.
	//
	// Event handlers are dispatched through a threadpool, and so multiple events can be
	// dispatched simultaneously.
	async fn message(&self, ctx: Context, msg: Message) {
		// ignore bots
		if msg.author.bot {
			return;
		}
		if msg.content.eq("d!help") {
			let mut say: String = String::default();
			say += "Denpabot help:\n```\n";
			say += "d!list - list the banned words and admins\n";
			say += "d!admin {mention} - add an administrator\n";
			say += "d!remove {number} - remove a banned word from the list\n";
			say += "d!add {word} - add a banned word to the list\n";
			say += "```";
			if let Err(why) = msg.channel_id.say(&ctx.http, say).await {
				println!("Error listing banned words: {why:?}");
			}
			return;
		}
		if msg.content.eq("d!list") {
			self.say_list(&ctx, &msg, false).await;
			return;
		}
		// in the list of admins
		if self.app_data.read().unwrap().admins.iter().find(|x| x.1 == msg.author.id.get()).is_some() {
			if msg.content.starts_with("d!admin") {
				for user in &msg.mentions {
					self.app_data.write().unwrap().admins.push((user.name.clone(), user.id.get()));
				}
				self.save();
				self.say_list(&ctx, &msg, true).await;
				return;
			}
			if msg.content.starts_with("d!remove ") {
				let num = msg.content.replace("d!remove ", "");
				let idx = str::parse::<usize>(&num).unwrap() - 1;
				{
					let mut ad = self.app_data.write().unwrap();
					if idx < ad.words.len() {
						ad.words.remove(idx);
					}
				}
				self.save();
				self.say_list(&ctx, &msg, true).await;
				return;
			}
			if msg.content.starts_with("d!add ") {
				let phrase = msg.content.replace("d!add ", "");
				self.app_data.write().unwrap().words.push(phrase);
				self.save();
				self.say_list(&ctx, &msg, true).await;
				return;
			}
		}
		let content = msg.content.to_lowercase();
		let censors = self.censor_list.read().unwrap().find_matches(content.as_str());
		if !censors.is_empty() {
			let allows = self.allow_list.read().unwrap().find_matches(content.as_str());
			// check the censors to see if there's an allow around it
			let mut checks = censors.len();
			for censor in censors.iter() {
				for allow in allows.iter() {
					if allow.0 <= censor.0 && allow.1 >= censor.1 {
						checks -= 1;
						break;
					}
				}
			}
			if checks > 0 {
				if let Err(why) = msg.delete(&ctx.http).await {
					println!("Error deleting message: {why:?}");
				}
				let mut dm_msg = CreateMessage::default();
				dm_msg = dm_msg.content(format!("Your message was deleted:\n```\n{}\n```", msg.content));
				if let Err(why) = msg.author.dm(&ctx.http, dm_msg).await {
					println!("Error deleting message: {why:?}");
				}
			}
		}
	}

	// Set a handler to be called on the `ready` event. This is called when a shard is booted, and
	// a READY payload is sent by Discord. This payload contains data like the current user's guild
	// Ids, current user data, private channels, and more.
	//
	// In this case, just print what the current user's username is.
	async fn ready(&self, _: Context, ready: Ready) {
		println!("{} is connected!", ready.user.name);
	}
}

impl Handler {
	fn save(&self) {
		{
			let app_data_guard = self.app_data.read().unwrap();
			let app_data = serde_cbor::to_vec(&*app_data_guard).unwrap();
			std::fs::write(APP_DATA_FILE, app_data).unwrap();
		}
		self.build();
	}

	fn load(&mut self) {
		match std::fs::read(APP_DATA_FILE) {
			Ok(data) => {
				let mut app_data = self.app_data.write().unwrap();
				*app_data = serde_cbor::from_slice(&data[..]).unwrap();
			}
			Err(_) => {
				println!("Failed to load list.dat")
			}
		}
		self.build();
	}

	fn build(&self) {
		let timer = Instant::now();
		// rebuild the censor list
		let app_data_guard = self.app_data.read().unwrap();
		*self.censor_list.write().unwrap() = app_data_guard.build_trie();
		{
			let cl = self.censor_list.read().unwrap();
			let mut al = self.allow_list.write().unwrap();
			al.reset();
			for word in APP_WORD_LIST.lines() {
				if cl.find_word(word) {
					continue;
				}
				al.insert(word);
			}
		}
		let build_time = Instant::now() - timer;
		println!("It took {} seconds to build the lists", build_time.as_secs_f32());
	}

	async fn say_list(&self, ctx: &Context, msg: &Message, on_update: bool) {
		let mut say: String = String::default();
		if on_update {
			say += "Updated!\n";
		}
		say += "Banned word list:\n```\n";
		let mut x = 0;
		for (i, word) in self.app_data.read().unwrap().words.iter().enumerate() {
			let n = i + 1;
			say += format!("{n}. {word}\n").as_str();
			x += 1;
		}
		if x == 0 {
			say += "x";
		}
		say += "```\n";
		say += "Admin list:\n```\n";
		x = 0;
		for (i, admin) in self.app_data.read().unwrap().admins.iter().enumerate() {
			let n = i + 1;
			let name = &admin.0;
			say += format!("{n}. {name}\n").as_str();
			x += 1;
		}
		if x == 0 {
			say += "x";
		}
		say += "```";
		if let Err(why) = msg.channel_id.say(&ctx.http, say).await {
			println!("Error listing banned words: {why:?}");
		}
	}
}

#[tokio::main]
async fn main() {
	let mut handler = Handler::default();

	// hardcoded admin (me)
	handler.app_data.write().unwrap().admins.push(("mip5".to_string(), 231963552292929546));

	// Configure the client with your Discord bot token in the environment.
	let mut token: String = "".to_string();

	for path in ENV_PATHS {
		let key = std::fs::read_to_string(Path::new(path).join("key"));
		if key.is_ok() {
			token = key.unwrap();
		}
	}

	handler.load();

	// Set gateway intents, which decides what events the bot will be notified about
	let intents = GatewayIntents::GUILD_MESSAGES
		| GatewayIntents::DIRECT_MESSAGES
		| GatewayIntents::MESSAGE_CONTENT;

	// Create a new instance of the Client, logging in as a bot. This will automatically prepend
	// your bot token with "Bot ", which is a requirement by Discord for bot users.
	let mut client =
		Client::builder(&token, intents).event_handler(handler).await.expect("Err creating client");

	// Finally, start a single shard, and start listening to events.
	//
	// Shards will automatically attempt to reconnect, and will perform exponential backoff until
	// it reconnects.
	if let Err(why) = client.start().await {
		println!("Client error: {why:?}");
	}
}
