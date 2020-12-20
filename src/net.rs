use std::time;
use std::io::prelude::*;
use std::net::TcpListener;
use std::net::TcpStream;

pub struct Error{}

pub const MAX_SILENCE : time::Duration = time::Duration::from_secs(10);
pub fn invalid_tok(){
	eprintln!("Token didn't match to any peer");
}

pub type Token = u64;
pub type pck_size_t = u16;

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ClHello{
	pub username : String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SeHello{
	pub token : Token,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ClMesg{
	pub username : String,
	pub token : Token,
	pub mesg : String
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SeMesg{
	pub mesg : String
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SeInfo{
	pub users : Vec<String>
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SeUQuit{
	pub reason : String
}

//messages that a client might send
#[derive(serde::Deserialize, serde::Serialize)]
pub enum ClMessage{
	Hello(ClHello),//client first connects to server and sends this
	Mesg(ClMesg),//client sends this if wants to post
	IWantInfo(Token),//request from client of information about server
	IQuit(Token),//client notifies that he leaves
	Ping(Token)//sent to ensure the server that the client is here
}

//messages that a server sends
#[derive(serde::Deserialize, serde::Serialize)]
pub enum SeMessage{
	Hello(SeHello),//sent after client hello
	Mesg(SeMesg),//a message to the client
	Info(SeInfo),//server sent information abouts server
	UQuit(SeUQuit),//sent upon kicking from chat
}

pub enum PeerState{
	AwaitingAuth,
	Chatting,
	Quitting
}

enum BuilderState{
	GettingSize,
	GettingPacket
}

//struct for accumulating bytes and producing a message when it arrived completely
//first in the packet goes size as u16, then ClMesg enum serialized in bincode
pub struct MesgBuilder{
	pck_left : usize,
	size_left : usize,
	pck_buff : Vec<u8>, //for data
	size_buff : Vec<u8>, //for packet size
	state : BuilderState
}

impl MesgBuilder{
	pub fn new() -> Self{
		return MesgBuilder{
			pck_left : 0,
			size_left : std::mem::size_of::<pck_size_t>(),
			pck_buff : Vec::new(),
			size_buff : Vec::new(),
			state : BuilderState::GettingSize
		};
	}
	//this function eats a slice of u8, and returns messages it got from it
	//or None if there is no complete messages
	pub fn eat<RE_T>(&mut self, slice : &[u8] ) -> Option<Vec<RE_T>>
	where RE_T: for<'a> serde::Deserialize<'a>
	{
		let mut mesgs : Vec<RE_T> = Vec::new();
		let mut pos = 0usize;
		loop{
			match self.state{
			    BuilderState::GettingSize => {
					let mut to_read = self.size_left;
					if to_read > slice.len() - pos{
						to_read = slice.len() - pos;
					}
					self.size_buff.copy_from_slice(&slice[pos..pos+to_read]);
					pos += to_read;
					self.size_left -= to_read;

					if self.size_left == 0{
						self.state = BuilderState::GettingPacket;
						let pck_size : pck_size_t = bincode::deserialize(&self.size_buff[0..2]).unwrap();
						self.pck_left = pck_size as usize;
					}
				}
			    BuilderState::GettingPacket => {
					let mut to_read = self.pck_left;
					if to_read > slice.len() - pos{
						to_read = slice.len() - pos;
					}
					self.pck_buff.copy_from_slice(&slice[pos..pos+to_read]);
					pos += to_read;
					self.pck_left -= to_read;

					if self.pck_left == 0{
						self.state = BuilderState::GettingSize;
						self.size_left = std::mem::size_of::<pck_size_t>();
						let mesg = bincode::deserialize(&self.pck_buff[..]).unwrap();
						mesgs.push(mesg);
					}
				}
			}
			if pos == slice.len(){
				break;
			}
		}
		if mesgs.len() > 0{
			return Some(mesgs);
		}
		return None;
	}
}


//A peer struct
//TR_T is the type of message being transmitted, RE_T is the type being received.
//For server's peer (which is a client) TR_T would be SeMessage, and RE_T would be ClMessage,
//since server sends server's message, and receives client's message
pub struct Peer<RE_T>{
	pub username : String,
	pub token : Token,
	pub messages : std::collections::VecDeque<RE_T>,
	stream : TcpStream,
	pub state : PeerState,
	pub silent_from : time::Instant,
	builder : MesgBuilder
}

impl<RE_T> Peer<RE_T>
where RE_T: for<'a> serde::Deserialize<'a>
{
	pub fn connect(&mut self, addr : String) -> Result<(), Error>{
		let stream = match TcpStream::connect(addr){
		    Ok(v) => {v}
		    Err(_) => {return Result::Err(Error{});}
		};
		self.stream = stream;
		Ok(())
	}

	pub fn new(token : &Token, stream : TcpStream) -> Self{
		let peer = Peer{
			username : String::new(),
			token :  *token,
			messages : std::collections::VecDeque::new(),
			stream,
			state : PeerState::AwaitingAuth,
			silent_from : time::Instant::now(),
			builder : MesgBuilder::new()
		};
		return peer;
	}

	pub fn send<TR_T>(&mut self, mesg : &TR_T)
	where TR_T: serde::Serialize
	{
		let mesg_ser : Vec<u8> = match bincode::serialize(mesg){
		    Ok(val) => {val}
		    Err(_) => {
				eprintln!("Failed to serialize");
				return;}
		};
		//TODO - return error if the packet is too large and handle it
		if mesg_ser.len() > pck_size_t::MAX.into(){
			eprintln!("A packet is too large");
			return;
		}
		let size : pck_size_t = mesg_ser.len() as pck_size_t;
		let size = bincode::serialize(&size).expect("Failed to serialize");
		self.stream.write(&size[..]);
		self.stream.write(&mesg_ser[..]);
	}

	pub fn get_messages(&mut self){
		let mut buff = [0u8; 1024];
		match self.stream.peek(&mut buff){
		    Ok(n) => {
				if n == 0{
					return;	}
			}
		    Err(_) => {return;}
		};

		let n = match self.stream.read(&mut buff){
		    Ok(n) => n,
		    Err(_) => {return;}
		};

		let mut mesgs = match self.builder.eat::<RE_T>(&buff[0..n]){
		    Some(mesgs) => mesgs,
		    None => {return;}
		};
		while mesgs.len() > 0{
			let mesg = match mesgs.pop() {
			    Some(mesg) => {mesg}
			    None => {break;}
			};
			self.messages.push_back(mesg);
		}
	}

	pub fn keep(&mut self){
		self.silent_from = time::Instant::now();
	}
}
