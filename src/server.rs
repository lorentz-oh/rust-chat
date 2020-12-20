extern crate rand;
extern crate serde;
extern crate bincode;

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
	should_stop : bool
}

impl Server{
	pub fn new(rx : std::sync::mpsc::Receiver<TcpStream>) -> Self{
		let server = Server{
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
		let response = SeMessage::Hello(SeHello{token});
		match self.peers.get_mut(&token){
		    Some(peer) => {
				peer.send(&response);
				self.name_map.insert(mesg.username.clone(), token);
				peer.state = PeerState::Chatting;
				peer.token = token;
				peer.username = mesg.username.clone();
			}
		    None => {invalid_tok()}
		}

	}

	fn verify(&self, token : &Token, username : &String) -> bool{
		let peer = match self.peers.get(token){
		    Some(p) => {p}
		    None => {return false;}
		};
		if peer.username != *username{
			return false
		}
		return true;
	}

	fn broadcast(&mut self, mesg : &ClMesg){
		if !self.verify(&mesg.token, &mesg.username){
			return;
		}
		let text = mesg.username.clone() + ": " + &mesg.mesg[..];
		let mesg = SeMessage::Mesg(
			SeMesg{ mesg : text});
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
		let mesg = SeMessage::UQuit(SeUQuit{reason : reason.clone()});
		match self.peers.get_mut(&token){
		    Some(p) => {p.send(&mesg)}
		    None => {}
		}
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
		    ClMessage::Mesg(m) => {self.broadcast(&m)}
		    ClMessage::IWantInfo(m) => {self.send_info(m)}
		    ClMessage::IQuit(m) => {self.peers.remove(&m);}
		    ClMessage::Ping(m) => {self.keep_peer(m)}
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
		while self.should_stop {
			match self.rx.try_recv(){
			    Ok(peer) => {self.register(peer)}
			    Err(_) => {}
			}
			self.get_messages();
			self.process_messages();
		}
	}
}

fn listen(tx : std::sync::mpsc::Sender<TcpStream>){
	let listener = loop{
		print!("Enter adress to listen: ");
		let mut addr = String::new();
		std::io::stdin().read_line(&mut addr);
		let listener = match TcpListener::bind(addr){
		    Ok(n) => n,
		    Err(_) => {
				eprintln!("Couldn't bind to adress");
				continue;
			}
		};
		break listener;
	};

	for stream in listener.incoming(){
		let mut stream = match stream {
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

fn main() {
	let (tx, rx) = std::sync::mpsc::channel();
	let listening_thread = std::thread::spawn(move || {listen(tx)});
	let mut server = Server::new(rx);
	server.run();
}