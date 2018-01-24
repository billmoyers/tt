extern crate rusqlite;
extern crate time;
extern crate clap;
extern crate rpassword;
extern crate hyper;
extern crate hyper_tls;
#[macro_use]
extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate tokio_core;
extern crate futures;

mod cli;
mod teamwork;

use time::Timespec;
use rusqlite::Connection;

#[derive(Debug, Deserialize)]
struct Project {
	id: String,
	name: String
}

#[derive(Debug)]
struct Timeblock {
	id: String,
	project_id: String,
	start: Timespec,
	end: Option<Timespec>
}

#[derive(Debug)]
struct Error {
	rusqlite: Option<rusqlite::Error>,
	io: Option<std::io::Error>,
	hyper: Option<hyper::Error>,
}

impl std::convert::From<rusqlite::Error> for Error {
	fn from(e: rusqlite::Error) -> Error {
		Error { rusqlite: Some(e), io: None, hyper: None }
	}
}
impl std::convert::From<std::io::Error> for Error {
	fn from(e: std::io::Error) -> Error {
		Error { rusqlite: None, io: Some(e), hyper: None }
	}
}
impl std::convert::From<hyper::Error> for Error {
	fn from(e: hyper::Error) -> Error {
		Error { rusqlite: None, io: None, hyper: Some(e) }
	}
}

trait System<'a> {
	fn setup(&self) -> Result<(), Error>;
	fn projects(&self) -> Result<(), hyper::error::Error>;
}

impl<'a> System<'a> {
	fn upgrade(conn: &'a Connection, vto: i32, version: &Fn(&'a Connection, i32) -> Result<(), rusqlite::Error>) -> Result<i32, rusqlite::Error> {
		let mut stmt = conn.prepare("SELECT version FROM metadata")?;

		let vfrom: i32 = stmt.query_map(&[], |row| {
			row.get(0)
		}).unwrap().next().unwrap().unwrap();
	
		println!("Metadata contains version {}...", vfrom);

		for v in vfrom+1..vto+1 {
			println!("Updating to version {}...", v);
			match v {
				0 => {
					try!(conn.execute("
						CREATE TABLE metadata (
							version INT NOT NULL DEFAULT 0
						)
					", &[]));

					try!(conn.execute("
						INSERT INTO metadata (version) VALUES (0)
					", &[]));

					try!(conn.execute("
						CREATE TABLE project (
							id TEXT PRIMARY KEY,
							name TEXT NOT NULL	
						)
					", &[]));

					try!(conn.execute("
						CREATE TABLE timeblock (
							id TEXT PRIMARY KEY,
							project_id TEXT NOT NULL REFERENCES project(id),
							start TIMESTAMP NOT NULL,
							end TIMESTAMP	
						)
					", &[]));
				}
				_ => {
					return Err(rusqlite::Error::QueryReturnedNoRows);
				}
			}
			match version(conn, v) {
				Ok(_) => {
					()
				}
				Err(e) => {
					return Err(e)
				}
			}
			try!(conn.execute("
				UPDATE metadata SET version=?
			", &[&v]));
		}

		Ok(vto)
	}
}

fn dispatch(m: &clap::ArgMatches, s: &System) {
	match m.subcommand_name() {
		Some("setup") => {
			s.setup();
		}
		Some("projects") => {
			match s.projects() {
				Ok(_) => {
				}
				Err(e) => {
					panic!("Failed getting projects: {:?}", e);
				}
			}
		}
		_ => {
			panic!("No commands specified.");
		}
	}
}


fn main() {
    let m = cli::build_cli().get_matches();
	
	match std::env::home_dir() {
		Some(p) => {
			//TODO Handle --db option here.
			let mut path = std::path::PathBuf::from(p);
			path.push("/src/.tt.sqlite");
			match Connection::open(path.as_path()) {
				Ok(conn) => {
					match teamwork::Teamwork::new(&conn, path.as_path(), 1) {
						Err(e) => {
							panic!("Error: {:?}", e);
						}
						Ok(s) => {
							dispatch(&m, &s);
						}
					};
				}
				Err(e) => {
					panic!("Failed opening {:?}: {:?}", path, e);
				}
			}
		}
		None => {
			panic!("$HOME dir not found.");
		}
	};
}
