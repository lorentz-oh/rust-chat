extern crate rand;
extern crate serde;
extern crate bincode;

use std::sync::{Arc, Mutex};
use std::io::prelude::*;
use std::net::TcpListener;
use std::net::TcpStream;
use std::collections::HashMap;
use std::time;
mod net;
use net::*;

struct Server{
	name_map : HashMap<String, Token>, //a map from username to token
	peers : HashMap<Token, Peer<ClMessage>>, //key is token
	rx : std::sync::mpsc::Receiver<TcpStream>, //receiver of stream from listening_thread
	input_rx : std::sync::mpsc::Receiver<String>,
	should_stop : bool
}

impl Server{
	pub fn new(rx : std::sync::mpsc::Receiver<TcpStream>, input_rx : std::sync::mpsc::Receiver<String>,) -> Self{
		let server = Server{
			input_rx,
			rx,
			peers : HashMap::new(),
			should_stop : false,
		 	name_map : HashMap::new()};
		return server;
	}

	pub fn get_messages(&mut self){
		for (_, peer) in self.peers.iter_mut(){
			peer.get_messages();
		}
	}

	fn authorize(&mut self, token : Token, mesg : &ClHello){
		if !mesg.username.chars().all(char::is_alphanumeric){
			self.disconnect(token, &"Username contains illegal character".to_string());
			return;
		}
		if self.name_map.contains_key(&mesg.username){
			self.disconnect(token, &"Username is in use".to_string());
			return;
		}


		match self.peers.get_mut(&token){
		    Some(peer) => {
				match peer.state {
				    PeerState::AwaitingAuth => {}
				    PeerState::Chatting => {return;} //an attempt to authorize while authorized
				    PeerState::Quitting => {}
				}
				let response = SeMessage::Hello(SeHello{token});
				peer.send(&response);
				self.name_map.insert(mesg.username.clone(), token);
				peer.state = PeerState::Chatting;
				peer.token = token;
				peer.username = mesg.username.clone();
			}
		    None => {
				invalid_tok();
				return;
			}
		}
		self.broadcast(&format!("{} joined", mesg.username));
	}

	fn verify(&self, token : &Token) -> bool{
		match self.peers.get(token){
		    Some(_) => {
				return true;
			}
		    None => {
				println!("Counldn't find user with token {}", token);
				return false;
			}
		};

	}

	fn broadcast(&mut self, mesg : &String){
		println!("{}", mesg);
		let mesg = SeMessage::Mesg(
			SeMesg{ mesg : mesg.clone() });
		for (_, peer) in self.peers.iter_mut(){
			peer.send(&mesg);
		}
	}

	fn send_info(&mut self, token : Token){
		let mut users : Vec<String> = Vec::new();
		users.reserve(self.name_map.len());
		for (username, _) in self.name_map.iter_mut(){
			users.push(username.clone());
		}
		let mesg = SeMessage::Info(SeInfo{users});
		match self.peers.get_mut(&token){
			Some(p) => {p.send(&mesg)}
			None => {invalid_tok()}
		}
	}

	fn disconnect(&mut self, token : Token, reason : &String){
		let username = match self.peers.get(&token){
		    Some(v) => {v.username.clone()}
		    None => {"".to_string()}
		};
		let mesg = SeMessage::UQuit(SeUQuit{reason : reason.clone()});
		match self.peers.get_mut(&token){
		    Some(p) => {p.send(&mesg)}
		    None => {}
		}
		self.name_map.remove(&username);
		self.peers.remove(&token);
	}

	fn keep_peer(&mut self, token : Token){
		match self.peers.get_mut(&token){
		    Some(peer) => {peer.keep()}
		    None => {invalid_tok()}
		}
	}

	fn process_message(&mut self, token : Token, mesg : ClMessage){
		self.keep_peer(token); //reset last activity time upon recieving a message from peer
		match mesg {
		    ClMessage::Hello(m) => {self.authorize(token, &m)}
		    ClMessage::Mesg(m) => {
				if !self.verify(&m.token){
					return;
				}
				self.broadcast(&(m.username.clone() + ": " + m.mesg.as_str()));
			}
		    ClMessage::IWantInfo(m) => {self.send_info(m)}
		    ClMessage::IQuit(m) => {self.disconnect(m, &"".to_string());}
		    ClMessage::Ping(m) => {}
		}
	}

	pub fn process_messages(&mut self){
		let mut messages : Vec<(Token, ClMessage)> = Vec::new();
		for (token, peer) in self.peers.iter_mut(){
			while peer.messages.len() > 0{
				match peer.messages.pop_back() {
				    Some(mesg) => {messages.push((*token, mesg))}
				    None => {eprintln!("Impossible!")}
				}
			}
		}
		for (token, mesg) in messages{
			self.process_message(token, mesg);
		}
	}

	//this function should return a random token which is not already occupied
	pub fn make_token(&self) -> Token{
		loop{
			let token : Token = rand::random();
			if self.peers.contains_key(&token){
				continue;
			}else{
				return token;
			}
		}
	}

	pub fn register(&mut self, stream : TcpStream){
		let token = self.make_token();
		let peer = Peer::new(&token, stream);
		self.peers.insert(token, peer);
	}

	pub fn run(&mut self){

		while !self.should_stop {
			match self.rx.try_recv(){
			    Ok(peer) => {self.register(peer)}
			    Err(_) => {}
			}
			self.get_messages();
			self.process_messages();
			self.process_input();
			self.kick_inactive();
		}
	}

	pub fn kick_inactive(&mut self){
		let mut inactive = Vec::new();
		for (token, peer) in &mut self.peers{
			if peer.silent_from.elapsed() > MAX_SILENCE{
				inactive.push((*token, peer.username.clone()));
			}
		}
		for(_, username) in & inactive{
			self.broadcast(&format!("-- {} timed out --", username));
		}
		for (token, _) in & inactive{
			self.disconnect(*token, &"timed out".to_string());
		}
	}

	pub fn process_input(&mut self){
		let input = match self.input_rx.try_recv(){
		    Ok(v) => {v},
		    Err(_) => {return;}
		};
		let mut command = input.trim().to_string();
		let arg = match command.find(' ') {
		    Some(pos) => {command.split_off(pos)}
		    None => {"".to_string()}
		};

		//commands without arguments
		match command.as_str(){
			"/help" =>{Server::print_help();}
			"/stop" => {
				let reason = "Server closed".to_string();
				let mut tokens = Vec::new();
				for (token, _) in self.peers.iter_mut(){
					tokens.push(token.clone());
				}
				for token in tokens{
					self.disconnect(token, &reason)
				}
				self.should_stop = true;
			}
			"/kick" => {
				self.kick(arg);
			}
			"/say" =>{
				self.broadcast(&("Server -- ".to_string() + &arg[..]));
			}
			_ =>{println!("Unrecognized command. Try /help")}
		}
	}

	pub fn kick(&mut self, arg :String){
		let mut iter = arg.split_whitespace();
		let username = match iter.next(){
		    Some(v) => {v.trim()}
		    None => {
				eprintln!("No username provided");
				return;
			}
		};
		let mut reason = String::new();
		reason.reserve(arg.len());
		loop{
			match iter.next() {
			    Some(v) => {
					reason += v;
					reason += " ";
				}
			    None => {break;}
			}
		}
		let token = match self.name_map.get(username){
			Some(v) => *v,
			None =>{
				println!("No such user: {}", username);
				return;
			}
		};
		self.disconnect(token, &reason.to_string());
		self.broadcast(&format!("{} was disconnected for the reason: {}", username, reason));
	}

	pub fn print_help(){
		println!("--------------------
A list of availible commands:
/help - displays help on commands
/kick <username> - kicks a user with <username>
/say <message> - sends a message
/stop - stops the server
--------------------")
	}
}

fn listen(tx : std::sync::mpsc::Sender<TcpStream>, is_bound : Arc<Mutex<bool>>){
	let listener = loop{
		println!("Enter address to listen: ");
		let mut addr = String::new();
		std::io::stdin().read_line(&mut addr);
		let listener = match TcpListener::bind(addr.trim()){
		    Ok(n) => n,
		    Err(_) => {
				eprintln!("Couldn't bind to adress");
				continue;
			}
		};
		*is_bound.lock().unwrap() = true;
		break listener;
	};

	println!("Listening");
	println!("Type /help for a list of commands");
	for stream in listener.incoming(){
		let stream = match stream {
			Ok(val) => {val},
			Err(_) => {println!("Connection failed");
				continue;}
		};
		match tx.send(stream){
			Ok(_) => {}
			Err(_) => {return;}
		}
	}
}

fn get_input(tx : std::sync::mpsc::Sender<String>, is_bound : Arc<Mutex<bool>>){
	while !*is_bound.lock().unwrap(){
		//wait till it's bound
	}
	loop{
		let mut command = String::new();
		std::io::stdin().read_line(&mut command);
		match tx.send(command){
			Ok(_) => {continue;}
			Err(_) => {return;}
		}
	}
}

fn main() {
	let mut is_bound = Arc::new(Mutex::new(false)); //whether server was bound to an address
	let (cli_tx, cli_rx) = std::sync::mpsc::channel();
	let (tx, rx) = std::sync::mpsc::channel();
	let is_bound_clone = is_bound.clone();
	let input_thread = std::thread::spawn(move || {get_input(cli_tx, is_bound_clone)});
	let is_bound_clone = is_bound.clone();
	let listening_thread = std::thread::spawn(move || {listen(tx, is_bound_clone)});
	let mut server = Server::new(rx, cli_rx);
	server.run();
}
