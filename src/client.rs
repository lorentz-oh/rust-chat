mod net;
use net::*;
use std::sync::Mutex;

struct Client{
	should_stop : bool,
	server : Option<Peer<SeMessage>>,
	server_mutex : Mutex<u8>,
	token : Token,
	username : String,
}

impl Client{
	pub fn new() -> Self{
		return Client{
			should_stop : false,
			server : None,
			server_mutex : Mutex::new(0),
			token : 0,
			username : String::new()
		};
	}

	pub fn run(&mut self){
		while !self.should_stop(){
			self.process_input();
		}
	}

	pub fn process_input(&mut self){
		let mut command = String::new();
		std::io::stdin().read_line(&mut command);
		command.trim();
		let space_pos = match command.find(' '){
			Some(v) => v,
			None => {
				println!("Incorrect command");
				return;
			}
		};
		let arg = command.split_off(space_pos);

		match command.as_str(){
			"/help" => {self.print_help();}
			"/join" => {self.join(arg);}
			"/disconnect" => {self.disconnect();}
			"/say" => {self.say(arg);}
			"/exit" => {self.exit();}
			_ =>{println!("Unrecognized command. Try /help")}
		}
	}

	pub fn join(&mut self, arg: String){
		
		match &mut self.server{
		    Some(v) => {
				println!("Already connected to a server");
				return;
			}
		    None => {}
		}
		let mut iter = arg.split_whitespace();
		let addr = match iter.next(){
		    Some(v) => {v}
		    None => {
				eprintln!("No adress provided");
				return;
			}
		};
		let username = match iter.next() {
		    Some(v) => {v}
		    None => {
				eprintln!("No username provided");
				return;
			}
		};
		let stream = match std::net::TcpStream::connect(&addr){
		    Ok(v) => {v}
		    Err(e) => {
				eprintln!("Couldn't connect: {:?}", e);
				return;
			}
		};
		self.username = username.to_string();
		let mut peer : Peer<SeMessage> = Peer::new(&0u64, stream);
		let hello = ClMessage::Hello(ClHello{username : username.to_string()});
		peer.send(&hello);
		let started_at = std::time::Instant::now();
		loop{ //loop till something arrives
			peer.get_messages();
			//wait some time for server to respond
			if std::time::Duration::from_secs(10) > started_at.elapsed(){
				eprintln!("Timed out");
				return;
			}
			let response = match peer.messages.pop_back(){
			    Some(v) => {v}
			    None => {continue;}
			};
			match response{
			    SeMessage::Hello(msg) => {
					self.token = msg.token;
					break;
				}
			    _ => {
					eprintln!("Error: Expected hello from server, got something else");
					return;
				}
			}
		}
		self.server = Some(peer);
	}

	pub fn disconnect(&mut self){
		match &mut self.server {
			Some(v) => {
				v.send(&ClMessage::IQuit(self.token));
				self.server = None;
			}
			None => {
				println!("Not connected to a server");
			}
		}
	}

	pub fn say(&mut self, arg: String){
		let mesg = ClMesg{
				username : self.username.clone(),
				token : self.token,
				mesg : arg
		};
		match &mut self.server {
		    Some(v) => {v.send(&ClMessage::Mesg(mesg));}
		    None => {println!("Not connected to a server");}
		}
	}

	pub fn exit(&mut self){
		self.should_stop = true;
	}

	pub fn print_help(&self){
		println!("A list of availible command:\n
			/help - displays help on commands\n
			/join <adress> <username> - joins a server at <adress> with <username>\n
			/disconnect - disconnects from a server\n
			/say <message> - sends a message to the chat\n
			/exit - exits the program");
	}

	pub fn should_stop(&self) -> bool{
		return self.should_stop;
	}
}

fn main(){
	let mut client = Client::new();
	client.run();
}
