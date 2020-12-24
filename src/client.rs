mod net;
use net::*;
use std::time;
use std::sync::mpsc;

struct Client{
	should_stop : bool,
	server : Option<Peer<SeMessage>>,
	input_rx : mpsc::Receiver<String>
}

impl Client{
	pub fn new(input_rx : mpsc::Receiver<String>) -> Self{
		return Client{
			should_stop : false,
			server : None,
			input_rx
		};
	}

	pub fn run(&mut self){
		println!("For list of availible commands type /help");
		while !self.should_stop{
			self.process_input();
			self.process_messages();
		}
	}

	pub fn process_messages(&mut self){
		let mut server = match &mut self.server {
		    Some(v) => {v}
		    None => {return;}
		};
		server.get_messages();
		let mut messages : Vec<SeMessage> = Vec::new();
		loop{
			let msg = match server.messages.pop_back(){
			    Some(v) => {v}
			    None => {break;}
			};
			messages.push(msg);
		}
		for msg in messages{
			self.process_message(&msg);
		}
	}

	pub fn process_message(&mut self, msg : &SeMessage){
		match msg{
		    SeMessage::Mesg(v) => {
				println!("{}", v.mesg);
			}
		    SeMessage::Info(v) => {
				println!("--------------------");
				println!("There are {} users on the server:", v.users.len());
				for user in v.users.iter(){
					println!("- {}", user);
				}
				println!("--------------------");
			}
		    SeMessage::UQuit(v) => {
				println!("You was kicked for the reason: {}", v.reason);
				self.disconnect();
			}
			SeMessage::Hello(_) => {}//handled by join()
		}
	}

	pub fn process_input(&mut self){
		let mut input = match self.input_rx.try_recv(){
		    Ok(v) => {v},
		    Err(_) => {return;}
		};
		let mut command = input.trim().to_string();
		let arg = match command.find(' ') {
		    Some(pos) => {command.split_off(pos)}
		    None => {"".to_string()}
		};

		match command.as_str() {
			"/help" => {
				self.print_help();
			}
			"/exit" => {
				self.terminate();
			}
			"/disconnect" => {
				self.disconnect();
			}
			"/info" => {
				self.request_server_info();
			}
			"/join" => {self.join(arg);}
			"/say" => {self.say(arg);}
			_ =>{println!("Unrecognized command. Try /help")}
		}
	}

	pub fn request_server_info(&mut self){
		let mut server = match &mut self.server {
			Some(v) => {v}
			None => {
				println!("Can't /info - not connected to a server");
				return;
			}
		};
		server.send(&ClMessage::IWantInfo(server.token));
	}

	pub fn join(&mut self, arg: String){
		match self.server{
		    Some(_) => {
				println!("Already connected to a server");
				return;
			}
		    None => {}
		};
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
		let mut peer : Peer<SeMessage> = Peer::new(&0u64, stream);
		peer.username = username.to_string();
		let hello = ClMessage::Hello(ClHello{username : username.to_string()});
		peer.send(&hello);
		let started_at = std::time::Instant::now();
		println!("Connecting...");
		loop{ //loop till something arrives
			peer.get_messages();
			//wait some time for server to respond
			if std::time::Duration::from_secs(10) < started_at.elapsed(){
				eprintln!("Timed out");
				return;
			}
			let response = match peer.messages.pop_back(){
			    Some(v) => {v}
			    None => {continue;}
			};
			match response{
			    SeMessage::Hello(msg) => {
					peer.token = msg.token;
					break;
				}
				SeMessage::UQuit(mesg) =>{
					eprintln!("Server refused in connection for the reason: {}", mesg.reason);
					return;
				}
			    _ => {
					eprintln!("Error: Expected hello from server, got something else");
					return;
				}
			}
		}
		println!("Connected");
		self.server = Some(peer);
	}

	pub fn disconnect(&mut self){
		match &mut self.server {
			Some(server) => {
				server.send(&ClMessage::IQuit(server.token));
				self.server = None;
			}
			None => {
				println!("Not connected to a server");
			}
		}
	}

	pub fn terminate(&mut self){
		self.should_stop = true;
	}

	pub fn say(&mut self, arg: String){
		let mut server = match &mut self.server {
		    Some(v) => {v}
		    None => {
				println!("Can't /say - not connected to a server");
				return;
			}
		};
		let mesg = ClMesg{
				username : server.username.clone(),
				token : server.token,
				mesg : arg
		};
		server.send(&ClMessage::Mesg(mesg));
	}

	pub fn print_help(&self){
		println!("--------------------
A list of availible commands:
/help - displays help on commands
/join <adress> <username> - joins a server at <adress> with <username>
/disconnect - disconnects from a server
/say <message> - sends a message to the chat
/exit - exits the program
/info - prints information about server
--------------------");
	}

	pub fn get_input(tx : mpsc::Sender<String>){
		loop{
			let mut command = String::new();
			std::io::stdin().read_line(&mut command);
			match tx.send(command){
			    Ok(_) => {continue;}
			    Err(_) => {return;}
			}
		}
	}
}

fn main(){
	let (tx, rx) = mpsc::channel();
	let mut client = Client::new(rx);
	let join_handle = std::thread::spawn(move || {Client::get_input(tx);});
	client.run();
}
